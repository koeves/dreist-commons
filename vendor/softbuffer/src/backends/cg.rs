//! Softbuffer implementation using CoreGraphics.
use crate::backend_interface::*;
use crate::error::InitError;
use crate::{util, Rect, SoftBufferError};
use objc2::rc::Allocated;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Ivar, Sel};
use objc2::{msg_send, sel, ClassType, MainThreadMarker, Message};
use objc2_core_foundation::{CFRetained, CGPoint};
use objc2_core_graphics::{
    CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage, CGImageAlphaInfo,
    CGImageByteOrderInfo, CGImageComponentInfo, CGImagePixelFormatInfo,
};
use objc2_foundation::{
    ns_string, NSDictionary, NSKeyValueChangeKey, NSKeyValueChangeNewKey,
    NSKeyValueObservingOptions, NSNumber, NSObject, NSObjectNSKeyValueObserverRegistration,
    NSString, NSValue,
};
use objc2_quartz_core::{kCAGravityTopLeft, CALayer, CATransaction};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawWindowHandle};

use std::ffi::{c_void, CStr, CString};
use std::marker::PhantomData;
use std::mem::size_of;
use std::num::NonZeroU32;
use std::ops::Deref;
use std::ptr::{self, slice_from_raw_parts_mut, NonNull};
use std::sync::OnceLock;

/// The key-value observer that keeps our layer in sync with the root layer.
///
/// The Objective-C runtime has one class namespace per process, while every
/// dynamic library that links this crate (several plugins in one host, or two
/// formats of one plugin) has its own copy of this code. A fixed class name
/// made the second copy's registration fail, and reusing the first copy's
/// class would leave its methods dangling once that library is unloaded.
/// So each copy registers its own class, under a name unique to it.
struct ObserverClass {
    class: &'static AnyClass,
    layer: &'static Ivar,
}

// SAFETY: Classes and their instance variable descriptions are immutable
// once registered, and the runtime may be queried from any thread.
unsafe impl Send for ObserverClass {}
// SAFETY: Same as above.
unsafe impl Sync for ObserverClass {}

const OBSERVER_CLASS_PREFIX: &str = "SoftbufferObserver";
const LAYER_IVAR: &CStr = c"softbufferLayer";

fn observer_class() -> &'static ObserverClass {
    static CLASS: OnceLock<ObserverClass> = OnceLock::new();
    CLASS.get_or_init(|| {
        // The address of a static of this copy of the library makes the name
        // unique among the copies loaded at the same time. A counter steps past
        // the class of a copy that was loaded at that address and unloaded.
        let base = format!(
            "{OBSERVER_CLASS_PREFIX}_{:x}",
            ptr::addr_of!(CLASS) as usize
        );
        let mut builder = (0u32..)
            .find_map(|n| {
                let name = if n == 0 {
                    base.clone()
                } else {
                    format!("{base}_{n}")
                };
                let name = CString::new(name).expect("class name without NUL");
                ClassBuilder::new(&name, NSObject::class())
            })
            .expect("a free class name");
        builder.add_ivar::<*mut CALayer>(LAYER_IVAR);
        // SAFETY: The signatures match those of `-[NSObject dealloc]` and
        // `-[NSObject observeValueForKeyPath:ofObject:change:context:]`.
        unsafe {
            builder.add_method(
                sel!(dealloc),
                observer_dealloc as unsafe extern "C-unwind" fn(_, _),
            );
            builder.add_method(
                sel!(observeValueForKeyPath:ofObject:change:context:),
                observer_observe_value as unsafe extern "C-unwind" fn(_, _, _, _, _, _),
            );
        }
        let class = builder.register();
        let layer = class
            .instance_variable(LAYER_IVAR)
            .expect("the observer's layer ivar");
        ObserverClass { class, layer }
    })
}

/// An instance of the observer class.
#[derive(Debug)]
struct SendObserver(Retained<NSObject>);

// SAFETY: The observer only holds its retained layer, which it touches from
// key-value notifications (see `SendCALayer`).
unsafe impl Send for SendObserver {}
// SAFETY: Same as above.
unsafe impl Sync for SendObserver {}

impl Deref for SendObserver {
    type Target = NSObject;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// A new observer that updates `layer`, which it retains.
fn new_observer(layer: &CALayer) -> SendObserver {
    let cls = observer_class();
    // SAFETY: The class is a subclass of `NSObject`.
    let observer: Retained<NSObject> = unsafe {
        let observer: Allocated<NSObject> = msg_send![cls.class, alloc];
        msg_send![observer, init]
    };
    // SAFETY: The ivar is `*mut CALayer` and belongs to the observer's class;
    // the retained layer is released in `observer_dealloc`.
    unsafe { *cls.layer.load_ptr::<*mut CALayer>(&observer) = Retained::into_raw(layer.retain()) };
    SendObserver(observer)
}

unsafe extern "C-unwind" fn observer_dealloc(this: *mut AnyObject, _cmd: Sel) {
    let cls = observer_class();
    // SAFETY: `this` is an instance of the observer class, whose ivar holds a
    // retained layer (or null if `new_observer` did not get to set it).
    unsafe {
        let layer = *cls.layer.load_ptr::<*mut CALayer>(&*this);
        drop(Retained::from_raw(layer));
        let _: () = msg_send![super(this, NSObject::class()), dealloc];
    }
}

unsafe extern "C-unwind" fn observer_observe_value(
    this: *mut AnyObject,
    _cmd: Sel,
    key_path: *mut NSString,
    _object: *mut AnyObject,
    change: *mut NSDictionary<NSKeyValueChangeKey, AnyObject>,
    _context: *mut c_void,
) {
    // SAFETY: `this` is an instance of the observer class, whose ivar holds
    // the retained layer once observation starts; the arguments are null or
    // valid for the duration of the call.
    unsafe {
        let layer = *observer_class().layer.load_ptr::<*mut CALayer>(&*this);
        if let Some(layer) = layer.as_ref() {
            observer_update(layer, key_path.as_ref(), change.as_ref());
        }
    }
}

fn observer_update(
    layer: &CALayer,
    key_path: Option<&NSString>,
    change: Option<&NSDictionary<NSKeyValueChangeKey, AnyObject>>,
) {
    let change =
        change.expect("requested a change dictionary in `addObserver`, but none was provided");
    let new = change
        .objectForKey(unsafe { NSKeyValueChangeNewKey })
        .expect("requested change dictionary did not contain `NSKeyValueChangeNewKey`");

    // NOTE: Setting these values usually causes a quarter second animation to occur, which is
    // undesirable.
    //
    // However, since we're setting them inside an observer, there already is a transaction
    // ongoing, and as such we don't need to wrap this in a `CATransaction` ourselves.

    if key_path == Some(ns_string!("contentsScale")) {
        let new = new.downcast::<NSNumber>().unwrap();
        let scale_factor = new.as_cgfloat();

        // Set the scale factor of the layer to match the root layer when it changes (e.g. if
        // moved to a different monitor, or monitor settings changed).
        layer.setContentsScale(scale_factor);
    } else if key_path == Some(ns_string!("bounds")) {
        let new = new.downcast::<NSValue>().unwrap();
        let bounds = new.get_rect().expect("new bounds value was not CGRect");

        // Set `bounds` and `position` so that the new layer is inside the superlayer.
        //
        // This differs from just setting the `bounds`, as it also takes into account any
        // translation that the superlayer may have that we'd want to preserve.
        layer.setFrame(bounds);
    } else {
        panic!("unknown observed keypath {key_path:?}");
    }
}

#[derive(Debug)]
pub struct CGImpl<D, W> {
    /// Our layer.
    layer: SendCALayer,
    /// The layer that our layer was created from.
    ///
    /// Can also be retrieved from `layer.superlayer()`.
    root_layer: SendCALayer,
    observer: SendObserver,
    color_space: CFRetained<CGColorSpace>,
    /// The width of the underlying buffer.
    width: usize,
    /// The height of the underlying buffer.
    height: usize,
    /// The pixels, kept between frames so a caller can redraw only what
    /// changed (`age` 1) instead of filling a new buffer every frame.
    pixels: util::PixelBuffer,
    /// `pixels` holds the last presented frame.
    presented: bool,
    window_handle: W,
    _display: PhantomData<D>,
}

impl<D, W> Drop for CGImpl<D, W> {
    fn drop(&mut self) {
        // SAFETY: Registered in `new`, must be removed before the observer is deallocated.
        unsafe {
            self.root_layer
                .removeObserver_forKeyPath(&self.observer, ns_string!("contentsScale"));
            self.root_layer
                .removeObserver_forKeyPath(&self.observer, ns_string!("bounds"));
        }
    }
}

impl<D: HasDisplayHandle, W: HasWindowHandle> SurfaceInterface<D, W> for CGImpl<D, W> {
    type Context = D;
    type Buffer<'a>
        = BufferImpl<'a, D, W>
    where
        Self: 'a;

    fn new(window_src: W, _display: &D) -> Result<Self, InitError<W>> {
        // `NSView`/`UIView` can only be accessed from the main thread.
        let _mtm = MainThreadMarker::new().ok_or(SoftBufferError::PlatformError(
            Some("can only access Core Graphics handles from the main thread".to_string()),
            None,
        ))?;

        let root_layer = match window_src.window_handle()?.as_raw() {
            RawWindowHandle::AppKit(handle) => {
                // SAFETY: The pointer came from `WindowHandle`, which ensures that the
                // `AppKitWindowHandle` contains a valid pointer to an `NSView`.
                //
                // We use `NSObject` here to avoid importing `objc2-app-kit`.
                let view: &NSObject = unsafe { handle.ns_view.cast().as_ref() };

                // Force the view to become layer backed
                let _: () = unsafe { msg_send![view, setWantsLayer: Bool::YES] };

                // SAFETY: `-[NSView layer]` returns an optional `CALayer`
                let layer: Option<Retained<CALayer>> = unsafe { msg_send![view, layer] };
                layer.expect("failed making the view layer-backed")
            }
            RawWindowHandle::UiKit(handle) => {
                // SAFETY: The pointer came from `WindowHandle`, which ensures that the
                // `UiKitWindowHandle` contains a valid pointer to an `UIView`.
                //
                // We use `NSObject` here to avoid importing `objc2-ui-kit`.
                let view: &NSObject = unsafe { handle.ui_view.cast().as_ref() };

                // SAFETY: `-[UIView layer]` returns `CALayer`
                let layer: Retained<CALayer> = unsafe { msg_send![view, layer] };
                layer
            }
            _ => return Err(InitError::Unsupported(window_src)),
        };

        // Add a sublayer, to avoid interfering with the root layer, since setting the contents of
        // e.g. a view-controlled layer is brittle.
        let layer = CALayer::new();
        root_layer.addSublayer(&layer);

        // Set the anchor point and geometry. Softbuffer's uses a coordinate system with the origin
        // in the top-left corner.
        //
        // NOTE: This doesn't really matter unless we start modifying the `position` of our layer
        // ourselves, but it's nice to have in place.
        layer.setAnchorPoint(CGPoint::new(0.0, 0.0));
        layer.setGeometryFlipped(true);

        // Do not use auto-resizing mask.
        //
        // This is done to work around a bug in macOS 14 and above, where views using auto layout
        // may end up setting fractional values as the bounds, and that in turn doesn't propagate
        // properly through the auto-resizing mask and with contents gravity.
        //
        // Instead, we keep the bounds of the layer in sync with the root layer using an observer,
        // see below.
        //
        // layer.setAutoresizingMask(kCALayerHeightSizable | kCALayerWidthSizable);

        let observer = new_observer(&layer);
        // Observe changes to the root layer's bounds and scale factor, and apply them to our layer.
        //
        // The previous implementation updated the scale factor inside `resize`, but this works
        // poorly with transactions, and is generally inefficient. Instead, we update the scale
        // factor only when needed because the super layer's scale factor changed.
        //
        // Note that inherent in this is an explicit design decision: We control the `bounds` and
        // `contentsScale` of the layer directly, and instead let the `resize` call that the user
        // controls only be the size of the underlying buffer.
        //
        // SAFETY: Observer deregistered in `Drop` before the observer object is deallocated.
        unsafe {
            root_layer.addObserver_forKeyPath_options_context(
                &observer,
                ns_string!("contentsScale"),
                NSKeyValueObservingOptions::New | NSKeyValueObservingOptions::Initial,
                ptr::null_mut(),
            );
            root_layer.addObserver_forKeyPath_options_context(
                &observer,
                ns_string!("bounds"),
                NSKeyValueObservingOptions::New | NSKeyValueObservingOptions::Initial,
                ptr::null_mut(),
            );
        }

        // Set the content so that it is placed in the top-left corner if it does not have the same
        // size as the surface itself.
        //
        // TODO(madsmtm): Consider changing this to `kCAGravityResize` to stretch the content if
        // resized to something that doesn't fit, see #177.
        layer.setContentsGravity(unsafe { kCAGravityTopLeft });

        // Initialize color space here, to reduce work later on.
        let color_space = CGColorSpace::new_device_rgb().unwrap();

        // Grab initial width and height from the layer (whose properties have just been initialized
        // by the observer using `NSKeyValueObservingOptionInitial`).
        let size = layer.bounds().size;
        let scale_factor = layer.contentsScale();
        let width = (size.width * scale_factor) as usize;
        let height = (size.height * scale_factor) as usize;

        Ok(Self {
            layer: SendCALayer(layer),
            root_layer: SendCALayer(root_layer),
            observer,
            color_space,
            width,
            height,
            pixels: util::PixelBuffer(Vec::new()),
            presented: false,
            _display: PhantomData,
            window_handle: window_src,
        })
    }

    #[inline]
    fn window(&self) -> &W {
        &self.window_handle
    }

    fn resize(&mut self, width: NonZeroU32, height: NonZeroU32) -> Result<(), SoftBufferError> {
        let (w, h) = (width.get() as usize, height.get() as usize);
        if (w, h) != (self.width, self.height) {
            self.presented = false;
        }
        self.width = w;
        self.height = h;
        Ok(())
    }

    fn buffer_mut(&mut self) -> Result<BufferImpl<'_, D, W>, SoftBufferError> {
        let len = self.width * self.height;
        if self.pixels.len() != len {
            self.pixels.0 = vec![0; len];
            self.presented = false;
        }
        Ok(BufferImpl { imp: self })
    }
}

#[derive(Debug)]
pub struct BufferImpl<'a, D, W> {
    imp: &'a mut CGImpl<D, W>,
}

impl<D: HasDisplayHandle, W: HasWindowHandle> BufferInterface for BufferImpl<'_, D, W> {
    fn width(&self) -> NonZeroU32 {
        NonZeroU32::new(self.imp.width as u32).unwrap()
    }

    fn height(&self) -> NonZeroU32 {
        NonZeroU32::new(self.imp.height as u32).unwrap()
    }

    #[inline]
    fn pixels(&self) -> &[u32] {
        &self.imp.pixels
    }

    #[inline]
    fn pixels_mut(&mut self) -> &mut [u32] {
        &mut self.imp.pixels
    }

    fn age(&self) -> u8 {
        u8::from(self.imp.presented)
    }

    fn present(self) -> Result<(), SoftBufferError> {
        unsafe extern "C-unwind" fn release(
            _info: *mut c_void,
            data: NonNull<c_void>,
            size: usize,
        ) {
            let data = data.cast::<u32>();
            let slice = slice_from_raw_parts_mut(data.as_ptr(), size / size_of::<u32>());
            // SAFETY: This is the same slice that we passed to `Box::into_raw` below.
            drop(unsafe { Box::from_raw(slice) })
        }

        // The layer keeps the image's pixels: it gets a copy, and the kept
        // buffer stays for the next frame.
        let data_provider = {
            let len = self.imp.pixels.len() * size_of::<u32>();
            let buffer: *mut [u32] = Box::into_raw(Box::<[u32]>::from(&self.imp.pixels.0[..]));
            // Convert slice pointer to thin pointer.
            let data_ptr = buffer.cast::<c_void>();

            // SAFETY: The data pointer and length are valid.
            // The info pointer can safely be NULL, we don't use it in the `release` callback.
            unsafe {
                CGDataProvider::with_data(ptr::null_mut(), data_ptr, len, Some(release)).unwrap()
            }
        };

        // `CGBitmapInfo` consists of a combination of `CGImageAlphaInfo`, `CGImageComponentInfo`
        // `CGImageByteOrderInfo` and `CGImagePixelFormatInfo` (see e.g. `CGBitmapInfoMake`).
        //
        // TODO: Use `CGBitmapInfo::new` once the next version of objc2-core-graphics is released.
        let bitmap_info = CGBitmapInfo(
            CGImageAlphaInfo::NoneSkipFirst.0
                | CGImageComponentInfo::Integer.0
                | CGImageByteOrderInfo::Order32Little.0
                | CGImagePixelFormatInfo::Packed.0,
        );

        let image = unsafe {
            CGImage::new(
                self.imp.width,
                self.imp.height,
                8,
                32,
                self.imp.width * 4,
                Some(&self.imp.color_space),
                bitmap_info,
                Some(&data_provider),
                ptr::null(),
                false,
                CGColorRenderingIntent::RenderingIntentDefault,
            )
        }
        .unwrap();

        // The CALayer has a default action associated with a change in the layer contents, causing
        // a quarter second fade transition to happen every time a new buffer is applied. This can
        // be avoided by wrapping the operation in a transaction and disabling all actions.
        CATransaction::begin();
        CATransaction::setDisableActions(true);

        // SAFETY: The contents is `CGImage`, which is a valid class for `contents`.
        unsafe { self.imp.layer.setContents(Some(image.as_ref())) };

        CATransaction::commit();
        self.imp.presented = true;
        Ok(())
    }

    fn present_with_damage(self, _damage: &[Rect]) -> Result<(), SoftBufferError> {
        self.present()
    }
}

#[derive(Debug)]
struct SendCALayer(Retained<CALayer>);

// SAFETY: CALayer is dubiously thread safe, like most things in Core Animation.
// But since we make sure to do our changes within a CATransaction, it is
// _probably_ fine for us to use CALayer from different threads.
//
// See also:
// https://developer.apple.com/documentation/quartzcore/catransaction/1448267-lock?language=objc
// https://stackoverflow.com/questions/76250226/how-to-render-content-of-calayer-on-a-background-thread
unsafe impl Send for SendCALayer {}
// SAFETY: Same as above.
unsafe impl Sync for SendCALayer {}

impl Deref for SendCALayer {
    type Target = CALayer;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
