//! `#[tauri::command]` for the iOS core: the function stays a plain async fn.
use proc_macro::TokenStream;

#[proc_macro_attribute]
pub fn command(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}
