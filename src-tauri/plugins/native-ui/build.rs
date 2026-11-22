const COMMANDS: &[&str] = &["attach", "chrome", "menu", "sheet", "dismiss", "toast"];

fn main() {
  tauri_plugin::Builder::new(COMMANDS).ios_path("ios").build();
}
