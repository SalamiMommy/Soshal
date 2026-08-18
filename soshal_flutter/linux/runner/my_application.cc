#include "my_application.h"

#include <flutter_linux/flutter_linux.h>
#include <gio/gio.h>
#include <string.h>
#ifdef GDK_WINDOWING_X11
#include <gdk/gdkx.h>
#endif

#include "flutter/generated_plugin_registrant.h"

struct _MyApplication {
  GtkApplication parent_instance;
  char** dart_entrypoint_arguments;
};

G_DEFINE_TYPE(MyApplication, my_application, GTK_TYPE_APPLICATION)

// Called when first Flutter frame received.
static void first_frame_cb(MyApplication* self, FlView* view) {
  gtk_widget_show(gtk_widget_get_toplevel(GTK_WIDGET(view)));
}

// === XDG Desktop Portal location (`com.soshal/portal` MethodChannel) ===
//
// Asks org.freedesktop.portal.Location for a one-shot device fix. The portal
// daemon shows the per-app permission dialog (sandboxed builds) or grants
// directly (plain AppImage); result arrives asynchronously via the
// Request::Response / Session::LocationUpdated DBus signals.

typedef struct {
  GDBusConnection* connection;
  GDBusProxy* location;
  gchar* request_path;
  gchar* session_path;
  guint response_sub;
  guint location_sub;
  FlMethodCall* method_call;
  gboolean responded;
} PortalLocationCtx;

static void portal_location_cleanup(PortalLocationCtx* ctx) {
  if (ctx->response_sub != 0) {
    g_dbus_connection_signal_unsubscribe(ctx->connection, ctx->response_sub);
  }
  if (ctx->location_sub != 0) {
    g_dbus_connection_signal_unsubscribe(ctx->connection, ctx->location_sub);
  }
  g_clear_object(&ctx->location);
  g_clear_object(&ctx->connection);
  g_free(ctx->request_path);
  g_free(ctx->session_path);
  g_free(ctx);
}

static void portal_respond_success(PortalLocationCtx* ctx, double lat,
                                   double lng) {
  ctx->responded = TRUE;
  FlValue* map = fl_value_new_map();
  fl_value_set_string_take(map, "latitude", fl_value_new_float(lat));
  fl_value_set_string_take(map, "longitude", fl_value_new_float(lng));
  fl_method_call_respond(
      ctx->method_call,
      FL_METHOD_RESPONSE(fl_method_success_response_new(map)), nullptr);
  portal_location_cleanup(ctx);
}

static void portal_respond_fail(PortalLocationCtx* ctx, const gchar* code,
                                const gchar* message) {
  ctx->responded = TRUE;
  fl_method_call_respond(
      ctx->method_call,
      FL_METHOD_RESPONSE(
          fl_method_error_response_new(code, message, nullptr)),
      nullptr);
  portal_location_cleanup(ctx);
}

// "latitude,longitude" string → doubles.
static gboolean portal_parse_location(const gchar* loc, double* lat,
                                      double* lng) {
  if (loc == nullptr || loc[0] == '\0') return FALSE;
  const gchar* comma = strchr(loc, ',');
  if (comma == nullptr) return FALSE;
  gchar* lat_end = nullptr;
  gchar* lng_end = nullptr;
  double a = g_ascii_strtod(loc, &lat_end);
  double b = g_ascii_strtod(comma + 1, &lng_end);
  if (lat_end == loc || lng_end == comma + 1 || a < -90.0 || a > 90.0 ||
      b < -180.0 || b > 180.0) {
    return FALSE;
  }
  *lat = a;
  *lng = b;
  return TRUE;
}

static void portal_signal_cb(GDBusConnection* connection,
                             const gchar* sender_name,
                             const gchar* object_path,
                             const gchar* interface_name,
                             const gchar* signal_name, GVariant* parameters,
                             gpointer user_data) {
  PortalLocationCtx* ctx = static_cast<PortalLocationCtx*>(user_data);
  if (ctx->responded) return;

  if (g_strcmp0(interface_name, "org.freedesktop.portal.Request") == 0 &&
      g_strcmp0(object_path, ctx->request_path) == 0) {
    guint32 response = 0;
    GVariant* results = nullptr;
    g_variant_get(parameters, "(u@a{sv})", &response, &results);
    if (response != 0) {
      g_variant_unref(results);
      portal_respond_fail(ctx, "DENIED", "Location permission denied");
      return;
    }
    const gchar* loc = nullptr;
    g_variant_lookup(results, "location", "s", &loc);
    g_variant_unref(results);
    double lat = 0.0;
    double lng = 0.0;
    if (loc != nullptr && portal_parse_location(loc, &lat, &lng)) {
      portal_respond_success(ctx, lat, lng);
    } else {
      portal_respond_fail(ctx, "FAILED", "Portal returned no location");
    }
  } else if (g_strcmp0(interface_name, "org.freedesktop.portal.Session") ==
                 0 &&
             g_strcmp0(object_path, ctx->session_path) == 0) {
    const gchar* loc = nullptr;
    gdouble accuracy = 0.0;
    g_variant_get(parameters, "(sd)", &loc, &accuracy);
    double lat = 0.0;
    double lng = 0.0;
    if (portal_parse_location(loc, &lat, &lng)) {
      portal_respond_success(ctx, lat, lng);
    }
  }
}

static gchar* portal_handle_token(void) {
  return g_strdup_printf("soshal%08x%08x", g_random_int(), g_random_int());
}

static void portal_request_location(FlMethodCall* method_call) {
  GError* error = nullptr;
  GDBusConnection* connection =
      g_bus_get_sync(G_BUS_TYPE_SESSION, nullptr, &error);
  if (connection == nullptr) {
    fl_method_call_respond(
        method_call,
        FL_METHOD_RESPONSE(fl_method_error_response_new(
            "UNAVAILABLE", "No DBus session bus (portal unavailable)",
            nullptr)),
        nullptr);
    g_clear_error(&error);
    return;
  }
  GDBusProxy* location = g_dbus_proxy_new_sync(
      connection, G_DBUS_PROXY_FLAGS_DO_NOT_LOAD_PROPERTIES, nullptr,
      "org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop",
      "org.freedesktop.portal.Location", nullptr, &error);
  if (location == nullptr) {
    fl_method_call_respond(
        method_call,
        FL_METHOD_RESPONSE(fl_method_error_response_new(
            "UNAVAILABLE", "Location portal not available", nullptr)),
        nullptr);
    g_clear_error(&error);
    g_object_unref(connection);
    return;
  }

  GVariantBuilder opts;
  g_variant_builder_init(&opts, G_VARIANT_TYPE("a{sv}"));
  gchar* handle_token = portal_handle_token();
  gchar* session_token = portal_handle_token();
  g_variant_builder_add(&opts, "{sv}", "handle_token",
                        g_variant_new_string(handle_token));
  g_variant_builder_add(&opts, "{sv}", "session_handle_token",
                        g_variant_new_string(session_token));
  GVariant* reply = g_dbus_proxy_call_sync(
      location, "CreateSession", g_variant_new("(a{sv})", &opts),
      G_DBUS_CALL_FLAGS_NONE, -1, nullptr, &error);
  g_free(handle_token);
  g_free(session_token);
  if (reply == nullptr) {
    fl_method_call_respond(
        method_call,
        FL_METHOD_RESPONSE(fl_method_error_response_new(
            "UNAVAILABLE", "Location session failed", nullptr)),
        nullptr);
    g_clear_error(&error);
    g_object_unref(location);
    g_object_unref(connection);
    return;
  }
  const gchar* session_path = nullptr;
  g_variant_get(reply, "(o)", &session_path);
  gchar* session_path_copy = g_strdup(session_path);
  g_variant_unref(reply);

  g_variant_builder_init(&opts, G_VARIANT_TYPE("a{sv}"));
  handle_token = portal_handle_token();
  g_variant_builder_add(&opts, "{sv}", "handle_token",
                        g_variant_new_string(handle_token));
  g_free(handle_token);
  reply = g_dbus_proxy_call_sync(
      location, "Start", g_variant_new("(osa{sv})", session_path_copy, "", &opts),
      G_DBUS_CALL_FLAGS_NONE, -1, nullptr, &error);
  if (reply == nullptr) {
    fl_method_call_respond(
        method_call,
        FL_METHOD_RESPONSE(fl_method_error_response_new(
            "UNAVAILABLE", "Location request failed", nullptr)),
        nullptr);
    g_clear_error(&error);
    g_free(session_path_copy);
    g_object_unref(location);
    g_object_unref(connection);
    return;
  }
  const gchar* request_path = nullptr;
  g_variant_get(reply, "(o)", &request_path);
  gchar* request_path_copy = g_strdup(request_path);
  g_variant_unref(reply);

  PortalLocationCtx* ctx = g_new0(PortalLocationCtx, 1);
  ctx->connection = connection;
  ctx->location = location;
  ctx->request_path = request_path_copy;
  ctx->session_path = session_path_copy;
  ctx->method_call = method_call;
  ctx->response_sub = g_dbus_connection_signal_subscribe(
      connection, "org.freedesktop.portal.Desktop",
      "org.freedesktop.portal.Request", "Response", nullptr, nullptr,
      G_DBUS_SIGNAL_FLAGS_NONE, portal_signal_cb, ctx, nullptr);
  ctx->location_sub = g_dbus_connection_signal_subscribe(
      connection, "org.freedesktop.portal.Desktop",
      "org.freedesktop.portal.Session", "LocationUpdated", nullptr, nullptr,
      G_DBUS_SIGNAL_FLAGS_NONE, portal_signal_cb, ctx, nullptr);
}

static void portal_method_call_cb(FlMethodChannel* channel,
                                  FlMethodCall* method_call,
                                  gpointer user_data) {
  const gchar* method = fl_method_call_get_name(method_call);
  if (g_strcmp0(method, "requestLocation") == 0) {
    portal_request_location(method_call);
    return;
  }
  fl_method_call_respond(
      method_call,
      FL_METHOD_RESPONSE(fl_method_not_implemented_response_new()),
      nullptr);
}

// Implements GApplication::activate.
static void my_application_activate(GApplication* application) {
  MyApplication* self = MY_APPLICATION(application);
  GtkWindow* window =
      GTK_WINDOW(gtk_application_window_new(GTK_APPLICATION(application)));

  // Use a header bar when running in GNOME as this is the common style used
  // by applications and is the setup most users will be using (e.g. Ubuntu
  // desktop).
  // If running on X and not using GNOME then just use a traditional title bar
  // in case the window manager does more exotic layout, e.g. tiling.
  // If running on Wayland assume the header bar will work (may need changing
  // if future cases occur).
  gboolean use_header_bar = TRUE;
#ifdef GDK_WINDOWING_X11
  GdkScreen* screen = gtk_window_get_screen(window);
  if (GDK_IS_X11_SCREEN(screen)) {
    const gchar* wm_name = gdk_x11_screen_get_window_manager_name(screen);
    if (g_strcmp0(wm_name, "GNOME Shell") != 0) {
      use_header_bar = FALSE;
    }
  }
#endif
  if (use_header_bar) {
    GtkHeaderBar* header_bar = GTK_HEADER_BAR(gtk_header_bar_new());
    gtk_widget_show(GTK_WIDGET(header_bar));
    gtk_header_bar_set_title(header_bar, "soshal_flutter");
    gtk_header_bar_set_show_close_button(header_bar, TRUE);
    gtk_window_set_titlebar(window, GTK_WIDGET(header_bar));
  } else {
    gtk_window_set_title(window, "soshal_flutter");
  }

  gtk_window_set_default_size(window, 1280, 720);

  // Set the window icon from the bundled asset (bundle/data/flutter_assets/
  // assets/icon.png). Silent no-op when the bundle path is unavailable.
  {
    g_autofree gchar* exe_path = g_file_read_link("/proc/self/exe", nullptr);
    if (exe_path != nullptr) {
      g_autofree gchar* exe_dir = g_path_get_dirname(exe_path);
      g_autofree gchar* icon_path = g_build_filename(
          exe_dir, "data", "flutter_assets", "assets", "icon.png", nullptr);
      gtk_window_set_icon_from_file(window, icon_path, nullptr);
    }
  }

  g_autoptr(FlDartProject) project = fl_dart_project_new();
  fl_dart_project_set_dart_entrypoint_arguments(
      project, self->dart_entrypoint_arguments);

  FlView* view = fl_view_new(project);
  GdkRGBA background_color;
  // Background defaults to black, override it here if necessary, e.g. #00000000
  // for transparent.
  gdk_rgba_parse(&background_color, "#000000");
  fl_view_set_background_color(view, &background_color);
  gtk_widget_show(GTK_WIDGET(view));
  gtk_container_add(GTK_CONTAINER(window), GTK_WIDGET(view));

  // Show the window when Flutter renders.
  // Requires the view to be realized so we can start rendering.
  g_signal_connect_swapped(view, "first-frame", G_CALLBACK(first_frame_cb),
                           self);
  gtk_widget_realize(GTK_WIDGET(view));

  fl_register_plugins(FL_PLUGIN_REGISTRY(view));

  // XDG Desktop Portal location channel (`com.soshal/portal`).
  FlEngine* engine = fl_view_get_engine(view);
  FlBinaryMessenger* messenger = fl_engine_get_binary_messenger(engine);
  FlMethodChannel* portal_channel = fl_method_channel_new(
      messenger, "com.soshal/portal",
      FL_METHOD_CODEC(fl_json_method_codec_new()));
  fl_method_channel_set_method_call_handler(
      portal_channel, portal_method_call_cb, nullptr, nullptr);

  gtk_widget_grab_focus(GTK_WIDGET(view));
}

// Implements GApplication::local_command_line.
static gboolean my_application_local_command_line(GApplication* application,
                                                  gchar*** arguments,
                                                  int* exit_status) {
  MyApplication* self = MY_APPLICATION(application);
  // Strip out the first argument as it is the binary name.
  self->dart_entrypoint_arguments = g_strdupv(*arguments + 1);

  g_autoptr(GError) error = nullptr;
  if (!g_application_register(application, nullptr, &error)) {
    g_warning("Failed to register: %s", error->message);
    *exit_status = 1;
    return TRUE;
  }

  g_application_activate(application);
  *exit_status = 0;

  return TRUE;
}

// Implements GApplication::startup.
static void my_application_startup(GApplication* application) {
  // MyApplication* self = MY_APPLICATION(object);

  // Perform any actions required at application startup.

  G_APPLICATION_CLASS(my_application_parent_class)->startup(application);
}

// Implements GApplication::shutdown.
static void my_application_shutdown(GApplication* application) {
  // MyApplication* self = MY_APPLICATION(object);

  // Perform any actions required at application shutdown.

  G_APPLICATION_CLASS(my_application_parent_class)->shutdown(application);
}

// Implements GObject::dispose.
static void my_application_dispose(GObject* object) {
  MyApplication* self = MY_APPLICATION(object);
  g_clear_pointer(&self->dart_entrypoint_arguments, g_strfreev);
  G_OBJECT_CLASS(my_application_parent_class)->dispose(object);
}

static void my_application_class_init(MyApplicationClass* klass) {
  G_APPLICATION_CLASS(klass)->activate = my_application_activate;
  G_APPLICATION_CLASS(klass)->local_command_line =
      my_application_local_command_line;
  G_APPLICATION_CLASS(klass)->startup = my_application_startup;
  G_APPLICATION_CLASS(klass)->shutdown = my_application_shutdown;
  G_OBJECT_CLASS(klass)->dispose = my_application_dispose;
}

static void my_application_init(MyApplication* self) {}

MyApplication* my_application_new() {
  // Set the program name to the application ID, which helps various systems
  // like GTK and desktop environments map this running application to its
  // corresponding .desktop file. This ensures better integration by allowing
  // the application to be recognized beyond its binary name.
  g_set_prgname(APPLICATION_ID);

  return MY_APPLICATION(g_object_new(my_application_get_type(),
                                     "application-id", APPLICATION_ID, "flags",
                                     G_APPLICATION_NON_UNIQUE, nullptr));
}