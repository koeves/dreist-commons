//! Objective-C classes the toolkit registers (macOS).
//!
//! The runtime has one class namespace per process, while every plugin
//! binary that links this crate has its own copy of the code: two plugins in
//! one host, or the VST3 and CLAP builds of one, would register the same
//! name. Reusing the class another binary registered would run that binary's
//! methods, which dangle once it is unloaded; so each copy declares its own
//! classes, under names unique to it.

use objc::declare::ClassDecl;
use objc::runtime::Class;

/// Start declaring a subclass of `superclass` named `<prefix>_<suffix>`,
/// where the suffix is unique to this copy of the library (the address of
/// one of its statics) and steps past names already taken (a copy that was
/// loaded at the same address and unloaded). Call it once per class and keep
/// the registered class.
pub(crate) fn declare(prefix: &str, superclass: &Class) -> ClassDecl {
    static ANCHOR: u8 = 0;
    let base = format!("{prefix}_{:x}", std::ptr::addr_of!(ANCHOR) as usize);
    (0u32..)
        .find_map(|n| {
            let name = if n == 0 { base.clone() } else { format!("{base}_{n}") };
            ClassDecl::new(&name, superclass)
        })
        .expect("a free class name")
}

#[cfg(test)]
#[allow(unexpected_cfgs)]
mod tests {
    use super::*;
    use objc::{class, msg_send, sel, sel_impl};

    fn name(c: &Class) -> String {
        c.name().to_string()
    }

    #[test]
    fn names_are_unique_and_skip_taken_ones() {
        let ns_object = class!(NSObject);
        // Taken: the bare prefix (as a fixed-name scheme would register it) and
        // this copy's own first choice (as an unloaded copy at the same address
        // would have left it).
        let prefix = "DreistGuiTestClass";
        ClassDecl::new(prefix, ns_object).expect("bare name").register();
        let first = declare(prefix, ns_object).register();
        let second = declare(prefix, ns_object).register();
        assert!(name(first).starts_with(&format!("{prefix}_")));
        assert_ne!(name(first), prefix);
        assert_eq!(name(second), format!("{}_1", name(first)));
        // Both are usable classes.
        unsafe {
            let o: *mut objc::runtime::Object = msg_send![second, new];
            let _: () = msg_send![o, release];
        }
    }

    #[test]
    fn menu_and_drag_classes_ignore_same_named_classes() {
        let product = dreist_formats::profile::get().product;
        for fixed in [format!("{product}MenuTarget"), format!("{product}DragOutSource")] {
            if Class::get(&fixed).is_none() {
                ClassDecl::new(&fixed, class!(NSObject)).expect("fixed name").register();
            }
        }
        let menu = crate::menubar::target_class();
        let drag = crate::dragout::source_class();
        assert!(name(menu).starts_with(&format!("{product}MenuTarget_")));
        assert!(name(drag).starts_with(&format!("{product}DragOutSource_")));
        assert!(menu.instance_method(sel!(menuChosen:)).is_some());
        assert!(drag.instance_method(sel!(draggingSession:sourceOperationMaskForDraggingContext:)).is_some());
        // Registered once.
        assert!(std::ptr::eq(menu, crate::menubar::target_class()));
        assert!(std::ptr::eq(drag, crate::dragout::source_class()));
    }
}
