//! The page calls the Swift side directly (`plugin:native-ui|…`, see src/native.js); nothing runs on the desktop.
use tauri::{
  plugin::{Builder, TauriPlugin},
  Runtime,
};

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_native_ui);

pub fn init<R: Runtime>() -> TauriPlugin<R> {
  Builder::new("native-ui")
    .setup(|_app, _api| {
      #[cfg(target_os = "ios")]
      _api.register_ios_plugin(init_plugin_native_ui)?;
      Ok(())
    })
    .build()
}
