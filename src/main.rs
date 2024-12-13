#![feature(os_str_display)]

extern crate native_windows_gui as nwg;
#[macro_use]
extern crate native_windows_derive as nwd;

use anyhow::Result;

use nwg::NativeUi;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

struct Column {
    list_view: nwg::ListView,
    child_paths: Vec<String>,
    path: Option<String>,
    is_loading: bool,
    handler: nwg::EventHandler,
}

impl Column {
    fn switch_path(&mut self, path: Option<&str>) {
        self.path = path.clone().map(Into::into);
        self.list_view.set_redraw(false);
        self.list_view.clear();
        self.child_paths.clear();
        if let Some(path) = path {
            let mut i = 0;
            let entries: Vec<std::fs::DirEntry> = std::fs::read_dir(path).unwrap().map(|x| x.unwrap()).collect();
            self.list_view.set_item_count(TryInto::<u32>::try_into(entries.len()).unwrap());
            self.child_paths.reserve(entries.len());
            for entry in entries {
                self.list_view.insert_item(nwg::InsertListViewItem {
                    text: Some(entry.file_name().display().to_string()),
                    image: None,
                    index: Some(i),
                    column_index: 0,
                });
                self.child_paths.push(entry.path().display().to_string());
                i += 1;
            }
        }
        self.list_view.set_redraw(true);
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

    #[nwg_layout(parent: window, max_row: Some(2), spacing: 3)]
    grid_layout: nwg::GridLayout,

    columns: Rc<RefCell<VecDeque<Column>>>,
}

impl StaplerApp {
    fn reconcile_columns(&self, width: i32) {
        let mut desired_column_count = calculate_column_count(width);
        if desired_column_count <= 0 {
            desired_column_count = 1;
        }
        let mut columns = self.columns.borrow_mut();
        while TryInto::<i32>::try_into(columns.len()).unwrap() > desired_column_count {
            let destroyed = if columns.back().unwrap().path.is_none() {
                columns.pop_back()
            } else {
                columns.pop_front()
            }.unwrap();
            nwg::unbind_event_handler(&destroyed.handler);
            self.grid_layout.remove_child(destroyed.list_view.handle);
        }
        for i in TryInto::<i32>::try_into(columns.len()).unwrap() ..= desired_column_count {
            let mut list_view = nwg::ListView::default();
            nwg::ListView::builder()
                .double_buffer(true)
                .list_style(nwg::ListViewStyle::Detailed)
                .parent(&self.window)
                .build(&mut list_view)
                .expect("failed to build list view");
            list_view.insert_column("Name");
            self.grid_layout.add_child(TryInto::<u32>::try_into(i).unwrap(), 1, &list_view);
            let list_view_handle = list_view.handle;
            let columns_ = Rc::downgrade(&self.columns);
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
                            let i = TryInto::<usize>::try_into(i).unwrap();
                            let row_index = TryInto::<usize>::try_into(row_index).unwrap();
                            let path = columns[i].child_paths[row_index].clone();
                            columns[i + 1].switch_path(Some(&path));
                        }
                    },
                    _ => {}
                }
            });
            columns.push_back(Column {
                list_view,
                path: None,
                child_paths: Vec::new(),
                is_loading: false,
                handler,
            });
        }
    }
    fn switch_column(&self, i: i32, path: Option<&str>) {
        let mut columns = self.columns.borrow_mut();
        let idx = TryInto::<usize>::try_into(i).unwrap();
        columns[idx].switch_path(path);
    }

    fn on_window_init(&self) {
        self.reconcile_columns(DEFAULT_WIDTH);
        self.switch_column(0, Some("/"));
        self.window.set_visible(true);
    }
    fn on_window_close(&self) {
        self.reconcile_columns(0);
        nwg::stop_thread_dispatch();
    }
    fn on_window_size(&self) {
        self.reconcile_columns(TryInto::<i32>::try_into(self.window.size().0).unwrap());
    }
}

fn main() {
    nwg::init().unwrap();
    let _ = nwg::Font::set_global_family("Segoe UI");
    let _app = StaplerApp::build_ui(StaplerApp::default()).unwrap();
    nwg::dispatch_thread_events();
}
