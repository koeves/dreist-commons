// A silent CLAP instrument with the apps' bus layout (a stereo input, a main
// stereo output and 16 stereo sub outputs, notes in and out), built by
// packaging/auv2/test.sh to check the Audio Unit wrapper with auval.
#include <clap/clap.h>
#include <stdio.h>
#include <string.h>

#define SUBS 16

static const char *features[] = {CLAP_PLUGIN_FEATURE_INSTRUMENT, CLAP_PLUGIN_FEATURE_STEREO, NULL};
static const clap_plugin_descriptor_t desc = {
    .clap_version = CLAP_VERSION_INIT,
    .id = "rs.dreist.auv2-test",
    .name = "AuTest",
    .vendor = "Dreist",
    .url = "",
    .manual_url = "",
    .support_url = "",
    .version = "1.0.0",
    .description = "Audio Unit wrapper test",
    .features = features,
};

static uint32_t ports_count(const clap_plugin_t *p, bool is_input) {
    (void)p;
    return is_input ? 1 : 1 + SUBS;
}

static bool ports_get(const clap_plugin_t *p, uint32_t index, bool is_input, clap_audio_port_info_t *info) {
    (void)p;
    if (index >= ports_count(p, is_input))
        return false;
    memset(info, 0, sizeof(*info));
    info->id = index;
    info->channel_count = 2;
    info->port_type = CLAP_PORT_STEREO;
    info->in_place_pair = CLAP_INVALID_ID;
    if (is_input) {
        snprintf(info->name, sizeof(info->name), "Sampler input");
        info->flags = CLAP_AUDIO_PORT_IS_MAIN;
    } else if (index == 0) {
        snprintf(info->name, sizeof(info->name), "Main");
        info->flags = CLAP_AUDIO_PORT_IS_MAIN;
    } else {
        snprintf(info->name, sizeof(info->name), "Sub output %u", index);
    }
    return true;
}

static const clap_plugin_audio_ports_t audio_ports = {ports_count, ports_get};

static uint32_t notes_count(const clap_plugin_t *p, bool is_input) {
    (void)p;
    (void)is_input;
    return 1;
}

static bool notes_get(const clap_plugin_t *p, uint32_t index, bool is_input, clap_note_port_info_t *info) {
    (void)p;
    (void)is_input;
    if (index != 0)
        return false;
    memset(info, 0, sizeof(*info));
    info->supported_dialects = CLAP_NOTE_DIALECT_CLAP | CLAP_NOTE_DIALECT_MIDI;
    info->preferred_dialect = CLAP_NOTE_DIALECT_MIDI;
    snprintf(info->name, sizeof(info->name), "Notes");
    return true;
}

static const clap_plugin_note_ports_t note_ports = {notes_count, notes_get};

static bool p_init(const clap_plugin_t *p) { (void)p; return true; }
static void p_destroy(const clap_plugin_t *p) { (void)p; }
static bool p_activate(const clap_plugin_t *p, double sr, uint32_t min, uint32_t max) {
    (void)p; (void)sr; (void)min; (void)max;
    return true;
}
static void p_deactivate(const clap_plugin_t *p) { (void)p; }
static bool p_start(const clap_plugin_t *p) { (void)p; return true; }
static void p_stop(const clap_plugin_t *p) { (void)p; }
static void p_reset(const clap_plugin_t *p) { (void)p; }
static void p_main_thread(const clap_plugin_t *p) { (void)p; }

static clap_process_status p_process(const clap_plugin_t *p, const clap_process_t *proc) {
    (void)p;
    for (uint32_t o = 0; o < proc->audio_outputs_count; o++)
        for (uint32_t c = 0; c < proc->audio_outputs[o].channel_count; c++)
            memset(proc->audio_outputs[o].data32[c], 0, proc->frames_count * sizeof(float));
    return CLAP_PROCESS_CONTINUE;
}

static const void *p_extension(const clap_plugin_t *p, const char *id) {
    (void)p;
    if (!strcmp(id, CLAP_EXT_AUDIO_PORTS))
        return &audio_ports;
    if (!strcmp(id, CLAP_EXT_NOTE_PORTS))
        return &note_ports;
    return NULL;
}

static clap_plugin_t plugin = {
    .desc = &desc,
    .plugin_data = NULL,
    .init = p_init,
    .destroy = p_destroy,
    .activate = p_activate,
    .deactivate = p_deactivate,
    .start_processing = p_start,
    .stop_processing = p_stop,
    .reset = p_reset,
    .process = p_process,
    .get_extension = p_extension,
    .on_main_thread = p_main_thread,
};

static uint32_t f_count(const clap_plugin_factory_t *f) { (void)f; return 1; }
static const clap_plugin_descriptor_t *f_desc(const clap_plugin_factory_t *f, uint32_t i) {
    (void)f;
    return i == 0 ? &desc : NULL;
}
static const clap_plugin_t *f_create(const clap_plugin_factory_t *f, const clap_host_t *host, const char *id) {
    (void)f;
    (void)host;
    return strcmp(id, desc.id) ? NULL : &plugin;
}

static const clap_plugin_factory_t factory = {f_count, f_desc, f_create};

static bool e_init(const char *path) { (void)path; return true; }
static void e_deinit(void) {}
static const void *e_factory(const char *id) {
    return strcmp(id, CLAP_PLUGIN_FACTORY_ID) ? NULL : &factory;
}

CLAP_EXPORT const clap_plugin_entry_t clap_entry = {CLAP_VERSION_INIT, e_init, e_deinit, e_factory};
