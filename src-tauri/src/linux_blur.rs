//! KDE Plasma blur behind the window (Settings → Appearance → Window material, Linux).
//! Wayland: KWin's org_kde_kwin_blur on GTK's own surface. X11: the _KDE_NET_WM_BLUR_BEHIND_REGION property.
//! Other desktops have no such API: nothing happens there (the option is only offered on KDE).
use gtk::{gdk, glib, prelude::*};
use std::cell::RefCell;
use wayland_client::{
    backend::{Backend, ObjectId},
    delegate_noop,
    globals::{registry_queue_init, GlobalListContents},
    protocol::{wl_compositor::WlCompositor, wl_region::WlRegion, wl_registry::WlRegistry, wl_surface::WlSurface},
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
};
use wayland_protocols_plasma::blur::client::{org_kde_kwin_blur::OrgKdeKwinBlur, org_kde_kwin_blur_manager::OrgKdeKwinBlurManager};

/// Rectangles covering a rounded rectangle: one per pixel row in the corners (regions are rectangles only).
pub fn rounded_rects(x: i32, y: i32, w: i32, h: i32, r: i32) -> Vec<[i32; 4]> {
    let r = r.clamp(0, w.min(h) / 2);
    let mut out = Vec::with_capacity(2 * r as usize + 1);
    for i in 0..r {
        let dy = f64::from(r - i) - 0.5;
        let inset = (f64::from(r) - (f64::from(r * r) - dy * dy).sqrt()).round() as i32;
        out.push([x + inset, y + i, w - 2 * inset, 1]);
        out.push([x + inset, y + h - 1 - i, w - 2 * inset, 1]);
    }
    out.push([x, y + r, w, h - 2 * r]);
    out
}

struct St;
impl Dispatch<WlRegistry, GlobalListContents> for St {
    fn event(_: &mut Self, _: &WlRegistry, _: <WlRegistry as Proxy>::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}
delegate_noop!(St: ignore WlCompositor);
delegate_noop!(St: ignore WlRegion);
delegate_noop!(St: ignore OrgKdeKwinBlurManager);
delegate_noop!(St: ignore OrgKdeKwinBlur);

/// Our own queue on GTK's Wayland connection. The wl_surface is never kept: GTK destroys it when the
/// window is hidden (run in background) and makes a new one when it is shown again.
struct Kde {
    conn: Connection,
    queue: EventQueue<St>,
    manager: OrgKdeKwinBlurManager,
    compositor: WlCompositor,
    blur: Option<OrgKdeKwinBlur>,
}

thread_local! {
    static KDE: RefCell<Option<Kde>> = const { RefCell::new(None) };
}

/// Sets (`Some`, window coordinates in logical px) or removes (`None`) the blur. GTK main thread only.
pub fn apply(win: &gtk::ApplicationWindow, rects: Option<Vec<[i32; 4]>>, tries: u32) {
    let surface = win.window().filter(|w| w.is_visible());
    let Some(gw) = surface else {
        // Not mapped yet (startup, or just shown again): try a little later.
        if tries < 10 {
            let win = win.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(300), move || apply(&win, rects, tries + 1));
        }
        return;
    };
    let r = if gw.display().type_().name() == "GdkWaylandDisplay" { wayland(&gw, rects) } else { x11(&gw, rects) };
    if let Err(err) = r {
        eprintln!("flou KDE : {err:#}");
    }
}

fn x11(gw: &gdk::Window, rects: Option<Vec<[i32; 4]>>) -> anyhow::Result<()> {
    let atom = gdk::Atom::intern("_KDE_NET_WM_BLUR_BEHIND_REGION");
    match rects {
        Some(rects) => {
            let scale = gw.scale_factor(); // X11 properties are in device pixels
            let data: Vec<std::os::raw::c_ulong> = rects.iter().flatten().map(|v| (v * scale).max(0) as std::os::raw::c_ulong).collect();
            gdk::property_change(gw, &atom, &gdk::Atom::intern("CARDINAL"), 32, gdk::PropMode::Replace, gdk::ChangeData::ULongs(&data));
        }
        None => gdk::property_delete(gw, &atom),
    }
    Ok(())
}

fn wayland(gw: &gdk::Window, rects: Option<Vec<[i32; 4]>>) -> anyhow::Result<()> {
    use glib::translate::ToGlibPtr;
    let raw: *mut gdk::ffi::GdkWindow = gw.to_glib_none().0;
    let disp: *mut gdk::ffi::GdkDisplay = gw.display().to_glib_none().0;
    // SAFETY: both come from a live Wayland GdkWindow / GdkDisplay (checked by the caller).
    let surface_ptr = unsafe { gdk_wayland_sys::gdk_wayland_window_get_wl_surface(raw as *mut _) };
    if surface_ptr.is_null() {
        anyhow::bail!("pas de surface Wayland");
    }
    KDE.with_borrow_mut(|kde| {
        if kde.is_none() {
            // SAFETY: GDK's wl_display lives as long as the app.
            let display_ptr = unsafe { gdk_wayland_sys::gdk_wayland_display_get_wl_display(disp as *mut _) };
            let conn = Connection::from_backend(unsafe { Backend::from_foreign_display(display_ptr as *mut _) });
            let (globals, queue) = registry_queue_init::<St>(&conn)?;
            let qh = queue.handle();
            let manager = globals.bind::<OrgKdeKwinBlurManager, _, _>(&qh, 1..=1, ()).map_err(|_| anyhow::anyhow!("ce compositeur ne sait pas flouter (KDE Plasma uniquement)"))?;
            let compositor = globals.bind::<WlCompositor, _, _>(&qh, 1..=4, ())?;
            *kde = Some(Kde { conn, queue, manager, compositor, blur: None });
        }
        let k = kde.as_mut().unwrap();
        // SAFETY: GTK's current surface for this window, used right away.
        let id = unsafe { ObjectId::from_ptr(WlSurface::interface(), surface_ptr as *mut _) }?;
        let surface = WlSurface::from_id(&k.conn, id)?;
        let qh = k.queue.handle();
        if let Some(old) = k.blur.take() {
            old.release();
        }
        match rects {
            Some(rects) => {
                let region = k.compositor.create_region(&qh, ());
                for [x, y, w, h] in rects {
                    region.add(x, y, w, h);
                }
                let blur = k.manager.create(&surface, &qh, ());
                blur.set_region(Some(&region));
                blur.commit(); // applied with GTK's next commit of the surface (every frame)
                region.destroy();
                k.blur = Some(blur);
            }
            None => k.manager.unset(&surface),
        }
        k.conn.flush()?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounded_rects_follow_the_corners() {
        assert_eq!(rounded_rects(12, 12, 100, 50, 0), vec![[12, 12, 100, 50]]);
        let r = rounded_rects(0, 0, 100, 50, 8);
        assert_eq!(r.len(), 17);
        assert_eq!(*r.last().unwrap(), [0, 8, 100, 34]);
        // Top row is the most inset, row 7 nearly full width, every row stays inside the box.
        assert!(r[0][0] > r[14][0]);
        assert!(r.iter().all(|[x, y, w, h]| *x >= 0 && *y >= 0 && x + w <= 100 && y + h <= 50));
    }
}
