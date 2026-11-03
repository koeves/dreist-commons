/*
 * dreist-rex-helper: decodes a REX file with Propellerhead's
 * "REX Shared Library.bundle" (installed by the user; the 1.7 library has
 * no arm64 slice, so this helper is built for x86_64 and runs under
 * Rosetta).
 *
 * Usage: dreist-rex-helper <file>
 * Output (stdout, little endian): "GRX1", then int32 channels, sample rate,
 * slice count, tempo, original tempo, PPQ length, time signature numerator
 * and denominator, bit depth (REXInfo); then per slice int32 PPQ position,
 * int32 frame count and the frames as float32, channel by channel.
 * Exit status: 0 ok, 2 library not found, 3 open failed, 4 file error,
 * 5 REX error.
 *
 * The calls are the ones the original makes ([re:c737bf]): REXCreate without callback, output rate 44100,
 * REXGetInfo (36 bytes), REXGetSliceInfo (8 bytes), REXRenderSlice.
 */
#include <CoreFoundation/CoreFoundation.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    int channels, sample_rate, slice_count, tempo, original_tempo, ppq_length, sign_num, sign_den, bit_depth;
} REXInfo;

typedef struct {
    int ppq_pos, sample_length;
} REXSliceInfo;

typedef char (*OpenProc)(void);
typedef void (*CloseProc)(void);
typedef int (*CreateProc)(void **handle, const char *buffer, int size, void *callback, void *user);
typedef void (*DeleteProc)(void **handle);
typedef int (*GetInfoProc)(void *handle, int size, REXInfo *info);
typedef int (*GetSliceInfoProc)(void *handle, int index, int size, REXSliceInfo *info);
typedef int (*SetRateProc)(void *handle, int rate);
typedef int (*RenderSliceProc)(void *handle, int index, int frames, float *out[2]);

static CFBundleRef open_bundle(void) {
    const char *home = getenv("HOME");
    char paths[2][1024];
    snprintf(paths[0], sizeof paths[0], "%s/Library/Application Support/Propellerhead Software/Rex/REX Shared Library.bundle", home ? home : "");
    snprintf(paths[1], sizeof paths[1], "/Library/Application Support/Propellerhead Software/Rex/REX Shared Library.bundle");
    for (int i = 0; i < 2; i++) {
        CFURLRef url = CFURLCreateFromFileSystemRepresentation(NULL, (const UInt8 *)paths[i], (CFIndex)strlen(paths[i]), true);
        if (!url) continue;
        CFBundleRef b = CFBundleCreate(NULL, url);
        CFRelease(url);
        if (b && CFBundleLoadExecutable(b)) return b;
        if (b) CFRelease(b);
    }
    return NULL;
}

static void *fn(CFBundleRef b, const char *name) {
    CFStringRef s = CFStringCreateWithCString(NULL, name, kCFStringEncodingUTF8);
    void *p = CFBundleGetFunctionPointerForName(b, s);
    CFRelease(s);
    return p;
}

static void put(int v) { fwrite(&v, 4, 1, stdout); }

int main(int argc, char **argv) {
    if (argc < 2) return 4;
    CFBundleRef b = open_bundle();
    if (!b) return 2;
    OpenProc open_ = (OpenProc)fn(b, "Open");
    CloseProc close_ = (CloseProc)fn(b, "Close");
    CreateProc create = (CreateProc)fn(b, "REXCreate");
    DeleteProc del = (DeleteProc)fn(b, "REXDelete");
    GetInfoProc info_ = (GetInfoProc)fn(b, "REXGetInfo");
    GetSliceInfoProc slice_info = (GetSliceInfoProc)fn(b, "REXGetSliceInfo");
    SetRateProc set_rate = (SetRateProc)fn(b, "REXSetOutputSampleRate");
    RenderSliceProc render = (RenderSliceProc)fn(b, "REXRenderSlice");
    if (!open_ || !close_ || !create || !del || !info_ || !slice_info || !set_rate || !render || !open_()) return 3;
    FILE *f = fopen(argv[1], "rb");
    if (!f) { close_(); return 4; }
    fseek(f, 0, SEEK_END);
    long size = ftell(f);
    fseek(f, 0, SEEK_SET);
    char *data = malloc((size_t)size);
    if (!data || fread(data, 1, (size_t)size, f) != (size_t)size) { fclose(f); close_(); return 4; }
    fclose(f);
    void *h = NULL;
    int status = 5;
    REXInfo info;
    memset(&info, 0, sizeof info);
    if (create(&h, data, (int)size, NULL, NULL) == 1 && set_rate(h, 44100) == 1 && info_(h, (int)sizeof info, &info) == 1) {
        fwrite("GRX1", 1, 4, stdout);
        put(info.channels); put(info.sample_rate); put(info.slice_count); put(info.tempo); put(info.original_tempo);
        put(info.ppq_length); put(info.sign_num); put(info.sign_den); put(info.bit_depth);
        status = 0;
        for (int i = 0; i < info.slice_count; i++) {
            REXSliceInfo si = {0, 0};
            if (slice_info(h, i, (int)sizeof si, &si) != 1) { status = 5; break; }
            float *buf[2] = { calloc((size_t)si.sample_length + 1, 4), calloc((size_t)si.sample_length + 1, 4) };
            if (render(h, i, si.sample_length, buf) != 1) { free(buf[0]); free(buf[1]); status = 5; break; }
            put(si.ppq_pos); put(si.sample_length);
            for (int c = 0; c < (info.channels == 2 ? 2 : 1); c++) fwrite(buf[c], 4, (size_t)si.sample_length, stdout);
            free(buf[0]); free(buf[1]);
        }
    }
    if (h) del(&h);
    free(data);
    close_();
    fflush(stdout);
    return status;
}
