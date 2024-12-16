#![feature(os_str_display)]

extern crate native_windows_gui as nwg;
#[macro_use]
extern crate native_windows_derive as nwd;

use anyhow::Result;

use nwg::NativeUi;

use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::ffi::{c_void, OsString};
use std::os::windows::ffi::OsStringExt;
use std::rc::Rc;

use windows::core::Interface;
use windows::Win32::UI::Shell as win32shell;
use windows::Win32::UI::Controls as win32controls;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::WindowsAndMessaging::{self as win32wam, HICON};

#[derive(Clone)]
enum Folder {
    Shell {
        sysobj: win32shell::IShellFolder,
        display: String,
        icon: Option<i32>,
    },
    Error(String),
}

#[derive(Debug)]
struct ItemId(*const win32shell::Common::ITEMIDLIST);
impl Drop for ItemId {
    fn drop(&mut self) {
        unsafe {
            CoTaskMemFree(Some(self.0 as *const c_void));
        }
    }
}

#[derive(Clone, Debug)]
enum File {
    Shell {
        itemid: Rc<ItemId>,
        display: String,
        icon: Option<i32>,
    },
    Error(String),
}

struct Column {
    /// The navigation icon
    proxy_icon: nwg::ImageFrame,
    /// The actual Windows columnview
    list_view: nwg::ListView,
    /// The path being shown in this column
    folder: Option<Folder>,
    /// A mirror list of child paths
    children: Vec<File>,
    /// If `Some`, a load is in progress
    loader: Option<nwg::Notice>,
    /// The event handler, bound to the list view
    handler: nwg::EventHandler,
}

impl Column {
    fn switch_into(&mut self, file: Option<File>, folder: Option<Folder>) {
        let file = if let Some(file) = file {
            file
        } else {
            self.switch(None);
            return;
        };
        match (file, folder) {
            (File::Error(err), _) | (_, Some(Folder::Error(err))) => {
                println!("{err:?}");
                return;
            }
            (File::Shell { itemid, display, icon }, Some(Folder::Shell { sysobj, display: _, icon: _ })) => {
                self.switch(Some(unsafe {
                    match sysobj.BindToObject(itemid.0, None) {
                        Ok(sysobj) => {
                            Folder::Shell {
                                display,
                                sysobj,
                                icon,
                            }
                        },
                        Err(e) => Folder::Error(format!("{e:?}")),
                    }
                }))
            }
            (File::Shell { itemid, display, icon }, None) => {
                self.switch(Some(unsafe {
                    match win32shell::SHGetDesktopFolder() {
                        Ok(sysobj) => {
                            match sysobj.BindToObject(itemid.0, None) {
                                Ok(sysobj) => Folder::Shell {
                                    display,
                                    sysobj,
                                    icon,
                                },
                                Err(e) => Folder::Error(format!("{e:?}")),
                            }
                        }
                        Err(e) => Folder::Error(format!("{e:?}")),
                    }
                }))
            }
        }
    }
    fn switch(&mut self, folder: Option<Folder>) {
        self.children.clear();
        self.list_view.clear();
        self.folder = folder.clone();
        if let Some(folder) = folder {
            let handle = self.list_view.handle.hwnd().unwrap() as usize;
            // jump to `StaplerApp::on_load_notice` for the rest of this
            match folder {
                Folder::Shell { sysobj, display: _, icon } => unsafe {
                    unsafe {
                        let mut big = win32controls::HIMAGELIST::default();
                        win32shell::Shell_GetImageLists(Some(&mut big), None);
                        let image_list_big = win32controls::IImageList::from_raw(big.0 as *mut _);
                        if let Some(hicon) = icon.and_then(|icon| image_list_big.GetIcon(icon, 0).ok()) {
                            println!("{hicon:?}");
                            self.proxy_icon.set_icon(Some(&nwg::Icon {
                                handle: hicon.0 as *mut _,
                                owned: false,
                            }));
                            self.proxy_icon.set_visible(true);
                        } else {
                            self.proxy_icon.set_visible(false);
                        };
                        std::mem::forget(image_list_big);
                    }
                    let mut penumidlist = None;
                    sysobj.EnumObjects(
                        windows::Win32::Foundation::HWND::default(),
                        TryInto::<u32>::try_into(win32shell::SHCONTF_FOLDERS.0 | win32shell::SHCONTF_NONFOLDERS.0).unwrap(),
                        &mut penumidlist,
                    );
                    if let Some(enumidlist) = penumidlist {
                        let mut rgelt = [std::ptr::null_mut(); 1];
                        let mut fetched_count = 0;
                        while enumidlist.Next(&mut rgelt[..], Some(&mut fetched_count)).is_ok() && fetched_count != 0 {
                            for i in 0..fetched_count {
                                let mut display_name_ret = win32shell::Common::STRRET::default();
                                sysobj.GetDisplayNameOf(rgelt[i as usize] as *const _, win32shell::SHGDN_INFOLDER, &mut display_name_ret);
                                let mut display_name_w = [0u16; 260];
                                win32shell::StrRetToBufW(&mut display_name_ret, Some(rgelt[i as usize] as *const _), &mut display_name_w);
                                self.children.push(File::Shell {
                                    itemid: Rc::new(ItemId(rgelt[i as usize])),
                                    display: OsString::from_wide(&display_name_w).display().to_string(),
                                    icon: Some(win32shell::SHMapPIDLToSystemImageListIndex(
                                        &sysobj,
                                        rgelt[i as usize],
                                        None,
                                    )),
                                });
                            }
                        }
                    }
                },
                Folder::Error(err) => {
                    self.proxy_icon.set_visible(false);
                    println!("{err:?}");
                },
            }
            self.list_view.set_redraw(false);
            self.list_view.set_item_count(TryInto::<u32>::try_into(self.children.len()).unwrap());
            let mut i = 0;
            for child in &self.children {
                let (text, image) = match child {
                    File::Shell { itemid: _, display, icon } => (display.clone(), *icon),
                    File::Error(string) => (format!("{string:?}"), None),
                };
                self.list_view.insert_item(nwg::InsertListViewItem {
                    text: Some(text),
                    image,
                    index: Some(i),
                    column_index: 0,
                });
                i += 1;
            }
            self.list_view.set_redraw(true);
        }
    }
}

const DEFAULT_WIDTH: i32 = 800;
const DEFAULT_HEIGHT: i32 = 600;
fn calculate_column_count(window_width: i32) -> i32 {
    (window_width / 300) + 1
}

#[derive(Default, NwgUi)]
pub struct StaplerApp {
    #[nwg_control(size: (DEFAULT_WIDTH, DEFAULT_HEIGHT), position: (300, 300), title: "Basic example", flags: "MAIN_WINDOW")]
    #[nwg_events(
        OnInit: [StaplerApp::on_window_init],
        OnResize: [StaplerApp::on_window_size],
        OnResizeEnd: [StaplerApp::on_window_size],
        OnWindowMaximize: [StaplerApp::on_window_size],
        OnWindowClose: [StaplerApp::on_window_close],
    )]
    window: nwg::Window,

    #[nwg_layout(parent: window, max_row: Some(1), spacing: 3, max_size: [u32::MAX, 64])]
    proxy_icon_grid_layout: nwg::GridLayout,

    #[nwg_layout(parent: window, max_row: Some(1), spacing: 3, margin: [64, 0, 0, 0])]
    column_grid_layout: nwg::GridLayout,

    image_list_small: RefCell<nwg::ImageList>,

    columns: Rc<RefCell<VecDeque<Column>>>,
}

impl StaplerApp {
    fn reconcile_columns(&self, desired_column_count: i32) {
        let mut columns = self.columns.borrow_mut();
        let needs_renumbered = TryInto::<i32>::try_into(columns.len()).unwrap() > desired_column_count;
        while TryInto::<i32>::try_into(columns.len()).unwrap() > desired_column_count {
            let destroyed = if columns.back().unwrap().folder.is_none() {
                columns.pop_back()
            } else {
                columns.pop_front()
            }.unwrap();
            nwg::unbind_event_handler(&destroyed.handler);
            self.proxy_icon_grid_layout.remove_child(destroyed.proxy_icon.handle);
            self.column_grid_layout.remove_child(destroyed.list_view.handle);
        }
        if needs_renumbered {
            let mut i = 0;
            for column in columns.iter_mut() {
                self.proxy_icon_grid_layout.remove_child(column.proxy_icon.handle);
                self.column_grid_layout.remove_child(column.list_view.handle);
                self.proxy_icon_grid_layout.add_child(i, 0, &column.proxy_icon);
                self.column_grid_layout.add_child(i, 0, &column.list_view);
                i += 1;
            }
        }
        let column_count = TryInto::<i32>::try_into(columns.len()).unwrap();
        std::mem::drop(columns);
        let icon = unsafe {
            let mut big = win32controls::HIMAGELIST::default();
            win32shell::Shell_GetImageLists(Some(&mut big), None);
            let image_list_big = win32controls::IImageList::from_raw(big.0 as *mut _);
            let result = if let Ok(hicon) = image_list_big.GetIcon(0, 0) {
                Some(nwg::Icon {
                    handle: hicon.0 as *mut _,
                    owned: false,
                })
            } else {
                None
            };
            std::mem::forget(image_list_big);
            result
        };
        for i in column_count .. desired_column_count {
            let mut proxy_icon = nwg::ImageFrame::default();
            nwg::ImageFrame::builder()
                .parent(&self.window)
                .size((64, 64))
                .icon(icon.as_ref())
                .build(&mut proxy_icon)
                .expect("failed");
            proxy_icon.set_visible(false);
            let mut list_view = nwg::ListView::default();
            nwg::ListView::builder()
                .double_buffer(true)
                .list_style(nwg::ListViewStyle::Detailed)
                .parent(&self.window)
                .build(&mut list_view)
                .expect("failed to build list view");
            list_view.set_image_list(Some(&self.image_list_small.borrow()), nwg::ListViewImageListType::Small);
            list_view.insert_column(nwg::InsertListViewColumn {
                index: None,
                fmt: None,
                width: Some(250),
                text: Some("Name".into()),
            });
            self.proxy_icon_grid_layout.add_child(TryInto::<u32>::try_into(i).unwrap(), 0, &proxy_icon);
            self.column_grid_layout.add_child(TryInto::<u32>::try_into(i).unwrap(), 0, &list_view);
            let list_view_handle = list_view.handle;
            let columns_ = Rc::downgrade(&self.columns);
            let proxy_icon_grid_layout = self.proxy_icon_grid_layout.clone();
            let column_grid_layout = self.column_grid_layout.clone();
            let handler = nwg::bind_event_handler(&list_view.handle, &self.window.handle, move |_evt, evt_data, handle| {
                let columns = if let Some(columns) = columns_.upgrade() {
                    columns
                } else {
                    return;
                };
                match evt_data {
                    nwg::EventData::OnListViewItemChanged { row_index, column_index: _, selected } => {
                        if handle == list_view_handle && selected {
                            let mut columns = columns.borrow_mut();
                            let mut column_iterator = columns.iter_mut();
                            let mut selected_path = None;
                            let mut parent_folder = None;
                            while let Some(column) = column_iterator.next() {
                                if column.list_view.handle == list_view_handle {
                                    selected_path = column.children.get(row_index).map(|x| x.to_owned());
                                    parent_folder = column.folder.clone();
                                    break;
                                }
                            }
                            while let Some(column) = column_iterator.next() {
                                column.switch_into(selected_path.take(), parent_folder.take());
                            }
                            // If the selected path hasn't been taken, it means we're at
                            // the right-most column.
                            if let Some(selected_path) = selected_path {
                                let mut surrogate = columns.pop_front().unwrap();
                                surrogate.switch_into(Some(selected_path), parent_folder);
                                columns.push_back(surrogate);
                                let mut i = 0;
                                for column in columns.iter() {
                                    proxy_icon_grid_layout.remove_child(&column.proxy_icon);
                                    proxy_icon_grid_layout.add_child(i, 0, &column.proxy_icon);
                                    column_grid_layout.remove_child(&column.list_view);
                                    column_grid_layout.add_child(i, 0, &column.list_view);
                                    i += 1;
                                }
                            }
                        }
                    },
                    _ => {}
                }
            });
            self.columns.borrow_mut().push_back(Column {
                proxy_icon,
                list_view,
                folder: None,
                children: Vec::new(),
                loader: None,
                handler,
            });
        }
    }
    fn switch_column(&self, i: i32, path: Option<Folder>) {
        let mut columns = self.columns.borrow_mut();
        let idx = TryInto::<usize>::try_into(i).unwrap();
        columns[idx].switch(path);
    }

    fn on_window_init(&self) {
        self.window.set_visible(true);
        let (sysobj, icon) = unsafe {
            let sysobj = win32shell::SHGetDesktopFolder().unwrap();
            let icon = match sysobj.cast::<win32shell::IPersistFolder2>().and_then(|ip| ip.GetCurFolder()) {
                Ok(cur_folder) => {
                    Some(win32shell::SHMapPIDLToSystemImageListIndex(
                        &sysobj,
                        cur_folder,
                        None,
                    ))
                }
                Err(_) => None,
            };
            (sysobj, icon)
        };
        let desktop = match sysobj.cast::<win32shell::IShellFolder>() {
            Ok(sysobj) => Folder::Shell {
                display: "Desktop".into(),
                sysobj,
                icon,
            },
            Err(e) => Folder::Error(format!("{e:?}")),
        };
        self.switch_column(0, Some(desktop));
    }
    fn on_window_close(&self) {
        self.reconcile_columns(0);
        nwg::stop_thread_dispatch();
    }
    fn on_window_size(&self) {
        if self.image_list_small.borrow().handle.is_null() {
            unsafe {
                let mut small = win32controls::HIMAGELIST::default();
                win32shell::Shell_GetImageLists(None, Some(&mut small));
                *self.image_list_small.borrow_mut() = nwg::ImageList {
                    owned: false,
                    handle: small.0 as *mut _,
                };
            }
        }
        let count = calculate_column_count(TryInto::<i32>::try_into(self.window.size().0).unwrap());
        self.reconcile_columns(count);
    }
}

fn main() {
    nwg::init().unwrap();
    let _ = nwg::Font::set_global_family("Segoe UI");
    let _app = StaplerApp::build_ui(StaplerApp::default()).unwrap();
    nwg::dispatch_thread_events();
}
