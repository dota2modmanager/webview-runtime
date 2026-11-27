/* wvr-selftest: draws a page with the bundle's WebKitGTK and checks what came out.
 *
 * The page's script paints the body green, so a green pixel proves the whole chain: GTK opened
 * the display, WebKit started its web and network processes (the patched helper path, and the
 * bundle's loader where the host has no glibc), JavaScriptCore ran the script, and the page was
 * drawn. Exit 0 on green; any other code says where it stopped.
 *
 * Built by linux/build.sh against the bundle's own WebKitGTK; run by `webview-runtime selftest`
 * or linux/smoke.sh, with a display (X11, Xwayland or Xvfb). */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <webkit2/webkit2.h>

static const char *PAGE =
    "<!doctype html><html><body style='margin:0;background:#ff0000'>"
    "<script>document.body.style.background = 'rgb(0, 255, 0)';</script></body></html>";

static void on_snapshot(GObject *view, GAsyncResult *res, gpointer data) {
    (void)data;
    GError *err = NULL;
    cairo_surface_t *s = webkit_web_view_get_snapshot_finish(WEBKIT_WEB_VIEW(view), res, &err);
    if (!s) {
        fprintf(stderr, "wvr-selftest: no snapshot: %s\n", err ? err->message : "?");
        exit(3);
    }
    cairo_surface_flush(s);
    if (cairo_surface_get_type(s) != CAIRO_SURFACE_TYPE_IMAGE) {
        fprintf(stderr, "wvr-selftest: snapshot is not an image surface\n");
        exit(3);
    }
    const unsigned char *data_ = cairo_image_surface_get_data(s);
    int stride = cairo_image_surface_get_stride(s);
    uint32_t px = *(const uint32_t *)(data_ + 20 * stride + 20 * 4); /* ARGB32, native order */
    unsigned r = (px >> 16) & 0xff, g = (px >> 8) & 0xff, b = px & 0xff;
    printf("wvr-selftest: WebKitGTK %u.%u.%u, pixel rgb(%u, %u, %u)\n", webkit_get_major_version(),
           webkit_get_minor_version(), webkit_get_micro_version(), r, g, b);
    if (g > 200 && r < 60 && b < 60) {
        printf("wvr-selftest: ok\n");
        exit(0);
    }
    fprintf(stderr, "wvr-selftest: the page did not come out green (%s)\n",
            r > 200 && g < 60 ? "drawn, but its script did not run" : "not drawn");
    exit(4);
}

static void on_load(WebKitWebView *view, WebKitLoadEvent event, gpointer data) {
    (void)data;
    if (event == WEBKIT_LOAD_FINISHED)
        webkit_web_view_get_snapshot(view, WEBKIT_SNAPSHOT_REGION_VISIBLE, WEBKIT_SNAPSHOT_OPTIONS_NONE,
                                     NULL, on_snapshot, NULL);
}

static gboolean on_failed(WebKitWebView *view, WebKitLoadEvent event, char *uri, GError *err, gpointer data) {
    (void)view; (void)event; (void)uri; (void)data;
    fprintf(stderr, "wvr-selftest: load failed: %s\n", err->message);
    exit(5);
}

static void on_crash(WebKitWebView *view, WebKitWebProcessTerminationReason reason, gpointer data) {
    (void)view; (void)data;
    fprintf(stderr, "wvr-selftest: the web process ended (%d)\n", reason);
    exit(6);
}

static gboolean on_timeout(gpointer data) {
    (void)data;
    fprintf(stderr, "wvr-selftest: nothing after 90 s\n");
    exit(7);
}

int main(int argc, char **argv) {
    if (!gtk_init_check(&argc, &argv)) {
        fprintf(stderr, "wvr-selftest: no display (set DISPLAY or WAYLAND_DISPLAY)\n");
        return 2;
    }
    GtkWidget *win = gtk_offscreen_window_new();
    gtk_window_set_default_size(GTK_WINDOW(win), 160, 120);
    GtkWidget *view = webkit_web_view_new();
    gtk_container_add(GTK_CONTAINER(win), view);
    g_signal_connect(view, "load-changed", G_CALLBACK(on_load), NULL);
    g_signal_connect(view, "load-failed", G_CALLBACK(on_failed), NULL);
    g_signal_connect(view, "web-process-terminated", G_CALLBACK(on_crash), NULL);
    g_timeout_add_seconds(90, on_timeout, NULL);
    webkit_web_view_load_html(WEBKIT_WEB_VIEW(view), PAGE, NULL);
    gtk_widget_show_all(win);
    gtk_main();
    return 1;
}
