//! The only Rust/Qt bridge in the native desktop shell.
//!
//! QML receives a bounded value list and generic state labels.  All controller
//! work is asynchronous and all background-to-Qt updates pass through
//! `CxxQtThread`, with a latest-value coalescer so a signal burst cannot create
//! an unbounded GUI callback queue.

use std::{pin::Pin, sync::Arc};

use cxx_qt::{casting::Upcast, CxxQtThread, CxxQtType, Threading};
use cxx_qt_lib::{QList, QMap, QMapPair_QString_QVariant, QString, QVariant};
use synveil_client::{
    BackgroundClientManager, BackgroundLaunchResult, DesktopController, DesktopControllerConfig,
    LibraryId, ServerProfileId,
};
use tokio::runtime::{Builder, Handle, Runtime};

use crate::{actions, presentation, profile};
use actions::{LatestValue, SyncRequestGate};
use presentation::{UiLibrary, UiSnapshot};

type QVariantList = QList<QVariant>;
type QVariantMap = QMap<QMapPair_QString_QVariant>;

#[cxx_qt::bridge]
pub mod ffi {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/core/qlist/qlist_QVariant.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;

        include!("cxx-qt-lib/qqmlapplicationengine.h");
        type QQmlApplicationEngine = cxx_qt_lib::QQmlApplicationEngine;

        include!("synveil-desktop/native/tray.h");
        type NativeApplication;
        type NativeTray;

        fn native_application_new() -> UniquePtr<NativeApplication>;
        fn native_application_set_metadata(
            app: Pin<&mut NativeApplication>,
            name: &QString,
            version: &QString,
        );
        fn native_application_exec(app: Pin<&mut NativeApplication>) -> i32;
        fn native_qml_engine_has_root(engine: &QQmlApplicationEngine) -> bool;

        fn native_tray_new(bridge: &QObject) -> UniquePtr<NativeTray>;
        fn native_tray_is_available(tray: &NativeTray) -> bool;
        fn native_tray_set_sync_enabled(tray: Pin<&mut NativeTray>, enabled: bool);
        fn native_tray_set_tooltip(tray: Pin<&mut NativeTray>, tooltip: &QString);
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QString, connection_label)]
        #[qproperty(QString, connection_detail)]
        #[qproperty(i32, connection_generation)]
        #[qproperty(QString, freshness_label)]
        #[qproperty(QString, process_label)]
        #[qproperty(QString, launch_label)]
        #[qproperty(QString, profile_message)]
        #[qproperty(QString, last_error_label)]
        #[qproperty(QVariant, libraries)]
        #[qproperty(i32, library_count)]
        #[qproperty(i32, attention_count)]
        #[qproperty(i32, root_unavailable_count)]
        #[qproperty(bool, libraries_truncated)]
        #[qproperty(QString, selected_library_id)]
        #[qproperty(QString, selected_library_label)]
        #[qproperty(QString, selected_runtime_label)]
        #[qproperty(QString, selected_root_label)]
        #[qproperty(QString, selected_auth_label)]
        #[qproperty(QString, selected_conflict_label)]
        #[qproperty(QString, selected_outcome_label)]
        #[qproperty(bool, selected_can_sync)]
        #[qproperty(bool, selected_needs_attention)]
        #[qproperty(bool, sync_in_flight)]
        #[qproperty(QString, sync_feedback)]
        #[qproperty(bool, tray_available)]
        #[qproperty(bool, profile_ready)]
        #[qproperty(bool, smoke_test)]
        #[qproperty(bool, live_test)]
        #[qproperty(bool, live_test_rapid_clicks)]
        #[qproperty(bool, live_test_exit_after_sync)]
        #[qproperty(bool, live_test_exit_after_ready)]
        #[qproperty(bool, live_test_exit_on_terminal)]
        type DesktopUiBridge = super::DesktopUiBridgeRust;

        #[cxx_name = "startController"]
        #[qinvokable]
        fn start_controller(self: Pin<&mut Self>);

        #[cxx_name = "installTray"]
        #[qinvokable]
        fn install_tray(self: Pin<&mut Self>);

        #[cxx_name = "selectLibrary"]
        #[qinvokable]
        fn select_library(self: Pin<&mut Self>, library_id: QString);

        #[cxx_name = "syncNow"]
        #[qinvokable]
        fn sync_now(self: Pin<&mut Self>);

        #[cxx_name = "syncLibrary"]
        #[qinvokable]
        fn sync_library(self: Pin<&mut Self>, library_id: QString);

        #[cxx_name = "requestQuit"]
        #[qinvokable]
        fn request_quit(self: Pin<&mut Self>);

        #[cxx_name = "trayOpen"]
        #[qinvokable]
        fn tray_open(self: Pin<&mut Self>);

        #[cxx_name = "traySyncNow"]
        #[qinvokable]
        fn tray_sync_now(self: Pin<&mut Self>);

        #[cxx_name = "trayQuit"]
        #[qinvokable]
        fn tray_quit(self: Pin<&mut Self>);

        #[cxx_name = "openRequested"]
        #[qsignal]
        fn open_requested(self: Pin<&mut Self>);

        #[cxx_name = "quitRequested"]
        #[qsignal]
        fn quit_requested(self: Pin<&mut Self>);

        #[cxx_name = "quitFinished"]
        #[qsignal]
        fn quit_finished(self: Pin<&mut Self>);
    }

    impl cxx_qt::Threading for DesktopUiBridge {}
    impl cxx_qt::Initialize for DesktopUiBridge {}
}

/// Rust-owned state backing the QML object.  The Qt-generated properties above
/// are the only values that cross into QML.
pub struct DesktopUiBridgeRust {
    pub(crate) connection_label: QString,
    pub(crate) connection_detail: QString,
    pub(crate) connection_generation: i32,
    pub(crate) freshness_label: QString,
    pub(crate) process_label: QString,
    pub(crate) launch_label: QString,
    pub(crate) profile_message: QString,
    pub(crate) last_error_label: QString,
    pub(crate) libraries: QVariant,
    pub(crate) library_count: i32,
    pub(crate) attention_count: i32,
    pub(crate) root_unavailable_count: i32,
    pub(crate) libraries_truncated: bool,
    pub(crate) selected_library_id: QString,
    pub(crate) selected_library_label: QString,
    pub(crate) selected_runtime_label: QString,
    pub(crate) selected_root_label: QString,
    pub(crate) selected_auth_label: QString,
    pub(crate) selected_conflict_label: QString,
    pub(crate) selected_outcome_label: QString,
    pub(crate) selected_can_sync: bool,
    pub(crate) selected_needs_attention: bool,
    pub(crate) sync_in_flight: bool,
    pub(crate) sync_feedback: QString,
    pub(crate) tray_available: bool,
    pub(crate) profile_ready: bool,
    pub(crate) smoke_test: bool,
    pub(crate) live_test: bool,
    pub(crate) live_test_rapid_clicks: bool,
    pub(crate) live_test_exit_after_sync: bool,
    pub(crate) live_test_exit_after_ready: bool,
    pub(crate) live_test_exit_on_terminal: bool,
    live_test_rapid_click_count: u32,

    controller: DesktopController,
    launch_manager: BackgroundClientManager,
    runtime: Option<Runtime>,
    sync_gate: Arc<SyncRequestGate>,
    presented: UiSnapshot,
    selected_id: Option<String>,
    started: bool,
    stopping: bool,
    tray: Option<cxx::UniquePtr<ffi::NativeTray>>,
}

impl Default for DesktopUiBridgeRust {
    fn default() -> Self {
        let smoke_test = std::env::var_os("SYNVEIL_QML_SMOKE_TEST").is_some();
        let live_test = std::env::var_os("SYNVEIL_QML_LIVE_TEST").is_some();
        let live_test_rapid_clicks =
            std::env::var_os("SYNVEIL_QML_LIVE_TEST_RAPID_CLICKS").is_some();
        let live_test_exit_after_sync =
            std::env::var_os("SYNVEIL_QML_LIVE_TEST_EXIT_AFTER_SYNC").is_some();
        let live_test_exit_after_ready =
            std::env::var_os("SYNVEIL_QML_LIVE_TEST_EXIT_AFTER_READY").is_some();
        let live_test_exit_on_terminal =
            std::env::var_os("SYNVEIL_QML_LIVE_TEST_EXIT_ON_TERMINAL").is_some();
        let profile_id = profile::load_profile_id();
        let profile_ready = profile_id.is_ok();
        let profile_id = profile_id.unwrap_or_else(|_| ServerProfileId::new());
        let controller = DesktopController::new(DesktopControllerConfig::for_profile(profile_id));
        let launch_manager = BackgroundClientManager::for_profile(profile_id);
        let runtime = Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .ok();

        let mut presented = UiSnapshot::default();
        if !profile_ready {
            presented.connection_code = "profile_unavailable";
            presented.connection_label = "Profile unavailable";
            presented.connection_detail = "Configure a desktop profile before connecting.";
            presented.freshness_code = "unavailable";
            presented.freshness_label = "Status unavailable";
        } else if runtime.is_none() {
            presented.connection_code = "runtime_unavailable";
            presented.connection_label = "Desktop shell unavailable";
            presented.connection_detail = "The desktop shell could not start its local tasks.";
        }

        Self {
            connection_label: QString::default(),
            connection_detail: QString::default(),
            connection_generation: 0,
            freshness_label: QString::default(),
            process_label: QString::default(),
            launch_label: QString::from("Background launch is not requested."),
            profile_message: QString::from(if profile_ready {
                "Using the configured desktop profile."
            } else {
                "Desktop profile is not configured."
            }),
            last_error_label: QString::default(),
            libraries: QVariant::default(),
            library_count: 0,
            attention_count: 0,
            root_unavailable_count: 0,
            libraries_truncated: false,
            selected_library_id: QString::default(),
            selected_library_label: QString::default(),
            selected_runtime_label: QString::default(),
            selected_root_label: QString::default(),
            selected_auth_label: QString::default(),
            selected_conflict_label: QString::default(),
            selected_outcome_label: QString::default(),
            selected_can_sync: false,
            selected_needs_attention: false,
            sync_in_flight: false,
            sync_feedback: QString::default(),
            tray_available: false,
            profile_ready,
            smoke_test,
            live_test,
            live_test_rapid_clicks,
            live_test_exit_after_sync,
            live_test_exit_after_ready,
            live_test_exit_on_terminal,
            live_test_rapid_click_count: 0,
            controller,
            launch_manager,
            runtime,
            sync_gate: Arc::new(SyncRequestGate::default()),
            presented,
            selected_id: None,
            started: false,
            stopping: false,
            tray: None,
        }
    }
}

impl cxx_qt::Initialize for ffi::DesktopUiBridge {
    fn initialize(mut self: Pin<&mut Self>) {
        let snapshot = self.rust().presented.clone();
        apply_snapshot(self.as_mut(), snapshot);
    }
}

/// Coalesces latest-state publications before they enter the Qt event loop.
#[derive(Clone)]
struct SnapshotDispatcher {
    qt_thread: CxxQtThread<ffi::DesktopUiBridge>,
    latest: LatestValue<UiSnapshot>,
}

impl SnapshotDispatcher {
    fn new(qt_thread: CxxQtThread<ffi::DesktopUiBridge>) -> Self {
        Self {
            qt_thread,
            latest: LatestValue::default(),
        }
    }

    fn publish(&self, snapshot: UiSnapshot) {
        if self.latest.publish(snapshot) {
            self.enqueue();
        }
    }

    fn enqueue(&self) {
        let dispatcher = self.clone();
        let result = self.qt_thread.queue(move |mut object| {
            let (snapshot, should_enqueue) = dispatcher.latest.take();
            if let Some(snapshot) = snapshot {
                apply_snapshot(object.as_mut(), snapshot);
            }

            if should_enqueue {
                dispatcher.enqueue();
            }
        });

        if result.is_err() {
            self.latest.discard();
        }
    }
}

impl ffi::DesktopUiBridge {
    fn start_controller(mut self: Pin<&mut Self>) {
        let (profile_ready, started, runtime, controller, launch_manager) = {
            let state = self.rust();
            (
                state.profile_ready,
                state.started,
                state
                    .runtime
                    .as_ref()
                    .map(|runtime| runtime.handle().clone()),
                state.controller.clone(),
                state.launch_manager.clone(),
            )
        };
        if started {
            return;
        }
        self.as_mut().rust_mut().get_mut().started = true;

        let Some(runtime) = runtime else {
            self.as_mut()
                .set_sync_feedback(QString::from("Desktop shell tasks are unavailable."));
            return;
        };
        if !profile_ready {
            self.as_mut()
                .set_sync_feedback(QString::from("Desktop profile is not configured."));
            return;
        }

        let dispatcher = SnapshotDispatcher::new(self.as_ref().get_ref().qt_thread());
        let mut updates = controller.subscribe_state();
        runtime.spawn(async move {
            if controller.start().await.is_err() {
                dispatcher.publish(presentation::map_snapshot(&controller.snapshot()));
                return;
            }
            let launch_result = launch_manager.ensure_running().await;
            let launch_label = launch_result_label(launch_result);
            let _ = dispatcher.qt_thread.queue(move |mut object| {
                object
                    .as_mut()
                    .set_launch_label(QString::from(launch_label));
            });
            dispatcher.publish(presentation::map_snapshot(&controller.snapshot()));
            while updates.changed().await.is_ok() {
                let snapshot = updates.borrow_and_update().clone();
                dispatcher.publish(presentation::map_snapshot(&snapshot));
            }
        });
    }

    fn install_tray(mut self: Pin<&mut Self>) {
        if self.rust().tray.is_some() {
            return;
        }
        let bridge: &cxx_qt::QObject = self.as_ref().get_ref().upcast();
        let tray = ffi::native_tray_new(bridge);
        if tray.is_null() {
            self.as_mut().set_tray_available(false);
            return;
        }
        let available = ffi::native_tray_is_available(&tray);
        let close_disposition = actions::close_disposition(available);
        debug_assert_eq!(actions::TRAY_ACTION_LABELS.len(), 3);
        self.as_mut().rust_mut().get_mut().tray = Some(tray);
        self.as_mut().set_tray_available(matches!(
            close_disposition,
            actions::CloseDisposition::HideToTray
        ));
        update_tray(self);
    }

    fn select_library(mut self: Pin<&mut Self>, library_id: QString) {
        let library_id = String::from(library_id);
        let is_known = self
            .rust()
            .presented
            .libraries
            .iter()
            .any(|library| library.library_id == library_id);
        if !is_known {
            return;
        }
        self.as_mut().rust_mut().get_mut().selected_id = Some(library_id);
        update_selected_properties(self);
    }

    fn sync_now(self: Pin<&mut Self>) {
        request_sync(self);
    }

    fn sync_library(self: Pin<&mut Self>, library_id: QString) {
        request_sync_for(self, Some(String::from(library_id)));
    }

    fn request_quit(mut self: Pin<&mut Self>) {
        debug_assert_eq!(
            actions::DESKTOP_QUIT_SCOPE,
            actions::QuitScope::ControllerOnly
        );
        let (controller, runtime, already_stopping) = {
            let state = self.rust();
            (
                state.controller.clone(),
                state
                    .runtime
                    .as_ref()
                    .map(|runtime| runtime.handle().clone()),
                state.stopping,
            )
        };
        if already_stopping {
            return;
        }
        self.as_mut().rust_mut().get_mut().stopping = true;

        let Some(runtime) = runtime else {
            self.as_mut().quit_finished();
            return;
        };
        let qt_thread = self.as_ref().get_ref().qt_thread();
        runtime.spawn(async move {
            let _ = controller.stop().await;
            let _ = qt_thread.queue(|mut object| {
                object.as_mut().quit_finished();
            });
        });
    }

    fn tray_open(mut self: Pin<&mut Self>) {
        self.as_mut().open_requested();
    }

    fn tray_sync_now(self: Pin<&mut Self>) {
        request_sync(self);
    }

    fn tray_quit(mut self: Pin<&mut Self>) {
        self.as_mut().quit_requested();
    }
}

fn launch_result_label(result: BackgroundLaunchResult) -> &'static str {
    match result {
        BackgroundLaunchResult::AlreadyRunning => "Background service already running.",
        BackgroundLaunchResult::StartRequested => "Background service start requested.",
        BackgroundLaunchResult::StartedSupervised => {
            "Background service started under supervision."
        }
        BackgroundLaunchResult::StartedDirect => "Background service started.",
        BackgroundLaunchResult::AlreadyStarting => {
            "Background service start is already in progress."
        }
        BackgroundLaunchResult::NotInstalled => "Background service is not installed.",
        BackgroundLaunchResult::SupervisorUnavailable => {
            "Background service supervisor unavailable."
        }
        BackgroundLaunchResult::LaunchDenied => "Background service launch was denied.",
        BackgroundLaunchResult::UnsafeState => {
            "Background service launch blocked by an unsafe state."
        }
        BackgroundLaunchResult::Failed => "Background service could not be started.",
    }
}

fn apply_snapshot(mut object: Pin<&mut ffi::DesktopUiBridge>, snapshot: UiSnapshot) {
    if object.rust().live_test {
        eprintln!(
            "SYNVEIL-LIVE-UI STATE connection={} freshness={} process={} generation={} libraries={} root={} auth={} error={} can_sync={} tray={}",
            snapshot.connection_label,
            snapshot.freshness_label,
            snapshot.process_label,
            snapshot.connection_generation,
            snapshot.libraries.len(),
            snapshot
                .libraries
                .first()
                .map_or("No library selected", |library| library.root_label),
            snapshot
                .libraries
                .first()
                .map_or("No library selected", |library| library.auth_label),
            snapshot.last_error_label,
            snapshot.can_sync_any,
            object.rust().tray_available,
        );
    }
    let selected_id = {
        let state = object.rust();
        presentation::selected_library_id(state.selected_id.as_deref(), &snapshot)
    };
    object.as_mut().rust_mut().get_mut().presented = snapshot.clone();
    object.as_mut().rust_mut().get_mut().selected_id = selected_id;

    object
        .as_mut()
        .set_connection_label(QString::from(snapshot.connection_label));
    object
        .as_mut()
        .set_connection_detail(QString::from(snapshot.connection_detail));
    object.as_mut().set_connection_generation(
        i32::try_from(snapshot.connection_generation).unwrap_or(i32::MAX),
    );
    object
        .as_mut()
        .set_freshness_label(QString::from(snapshot.freshness_label));
    object
        .as_mut()
        .set_process_label(QString::from(snapshot.process_label));
    object
        .as_mut()
        .set_last_error_label(QString::from(snapshot.last_error_label));
    object
        .as_mut()
        .set_libraries(to_qvariant_list(&snapshot.libraries));
    object
        .as_mut()
        .set_library_count(safe_i32(snapshot.libraries.len()));
    object
        .as_mut()
        .set_attention_count(safe_i32(snapshot.attention_count));
    object
        .as_mut()
        .set_root_unavailable_count(safe_i32(snapshot.root_unavailable_count));
    object
        .as_mut()
        .set_libraries_truncated(snapshot.libraries_truncated);
    update_selected_properties(object.as_mut());
    update_tray(object);
}

fn update_selected_properties(mut object: Pin<&mut ffi::DesktopUiBridge>) {
    let selected = {
        let state = object.rust();
        state
            .selected_id
            .as_ref()
            .and_then(|selected_id| {
                state
                    .presented
                    .libraries
                    .iter()
                    .find(|library| &library.library_id == selected_id)
            })
            .cloned()
    };
    let selected_id = selected
        .as_ref()
        .map(|library| library.library_id.as_str())
        .unwrap_or("");
    object
        .as_mut()
        .set_selected_library_id(QString::from(selected_id));
    object.as_mut().set_selected_library_label(QString::from(
        selected
            .as_ref()
            .map_or("No library selected", |library| library.label.as_str()),
    ));
    object.as_mut().set_selected_runtime_label(QString::from(
        selected
            .as_ref()
            .map_or("No library selected", |library| library.runtime_label),
    ));
    object.as_mut().set_selected_root_label(QString::from(
        selected
            .as_ref()
            .map_or("No library selected", |library| library.root_label),
    ));
    object.as_mut().set_selected_auth_label(QString::from(
        selected
            .as_ref()
            .map_or("No library selected", |library| library.auth_label),
    ));
    object.as_mut().set_selected_conflict_label(QString::from(
        selected
            .as_ref()
            .map_or("No library selected", |library| library.conflict_label),
    ));
    object.as_mut().set_selected_outcome_label(QString::from(
        selected
            .as_ref()
            .map_or("No library selected", |library| library.outcome_label),
    ));
    object
        .as_mut()
        .set_selected_can_sync(selected.as_ref().is_some_and(|library| library.can_sync));
    object.as_mut().set_selected_needs_attention(
        selected
            .as_ref()
            .is_some_and(|library| library.needs_attention),
    );
}

fn request_sync(object: Pin<&mut ffi::DesktopUiBridge>) {
    request_sync_for(object, None);
}

fn request_sync_for(
    mut object: Pin<&mut ffi::DesktopUiBridge>,
    requested_library_id: Option<String>,
) {
    {
        let state = object.as_mut().rust_mut().get_mut();
        if state.live_test && state.live_test_rapid_clicks {
            state.live_test_rapid_click_count = state.live_test_rapid_click_count.saturating_add(1);
            let count = state.live_test_rapid_click_count;
            if count == 1_000 {
                eprintln!("SYNVEIL-LIVE-UI ACTION sync_now_clicks=1000");
            }
        }
    }
    let requested_library = requested_library_id.is_some();
    let (library_id, can_sync, gate, controller, runtime, live_test) = {
        let state = object.rust();
        let library_id = requested_library_id.or_else(|| state.selected_id.clone());
        (
            library_id.clone(),
            library_id
                .as_ref()
                .and_then(|selected_id| {
                    state
                        .presented
                        .libraries
                        .iter()
                        .find(|library| &library.library_id == selected_id)
                })
                .is_some_and(|library| library.can_sync),
            Arc::clone(&state.sync_gate),
            state.controller.clone(),
            state
                .runtime
                .as_ref()
                .map(|runtime| runtime.handle().clone()),
            state.live_test,
        )
    };

    let Some(library_id) = library_id else {
        object
            .as_mut()
            .set_sync_feedback(QString::from("Select a library to request sync."));
        return;
    };
    if !can_sync {
        object
            .as_mut()
            .set_sync_feedback(QString::from(if requested_library {
                "That library is no longer available."
            } else {
                "Sync is unavailable until current status is available."
            }));
        return;
    }
    if !gate.try_acquire() {
        object
            .as_mut()
            .set_sync_feedback(QString::from("A sync request is already being processed."));
        return;
    }
    let Some(runtime) = runtime else {
        gate.release();
        object
            .as_mut()
            .set_sync_feedback(QString::from("Desktop shell tasks are unavailable."));
        return;
    };

    object.as_mut().set_sync_in_flight(true);
    object
        .as_mut()
        .set_sync_feedback(QString::from("Requesting sync…"));
    update_tray(object.as_mut());
    if live_test {
        eprintln!("SYNVEIL-LIVE-UI ACTION sync_now");
    }
    let qt_thread = object.as_ref().get_ref().qt_thread();
    runtime.spawn(async move {
        let feedback = match library_id.parse::<LibraryId>() {
            Ok(library_id) => controller.sync_now(library_id).await.map_or(
                "Background service unavailable.",
                presentation::command_feedback,
            ),
            Err(_) => "That library is no longer available.",
        };
        if live_test {
            eprintln!("SYNVEIL-LIVE-UI RESULT feedback={feedback}");
        }
        gate.release();
        let _ = qt_thread.queue(move |mut object| {
            object.as_mut().set_sync_in_flight(false);
            object.as_mut().set_sync_feedback(QString::from(feedback));
            update_tray(object);
        });
    });
}

fn update_tray(mut object: Pin<&mut ffi::DesktopUiBridge>) {
    let (enabled, available, tooltip) = {
        let state = object.rust();
        (
            state.selected_id.as_ref().is_some_and(|selected_id| {
                state
                    .presented
                    .libraries
                    .iter()
                    .find(|library| &library.library_id == selected_id)
                    .is_some_and(|library| library.can_sync)
            }) && !state.sync_in_flight,
            state.tray_available,
            actions::tray_tooltip(state.presented.connection_label),
        )
    };
    if available {
        if let Some(tray) = object.as_mut().rust_mut().get_mut().tray.as_mut() {
            ffi::native_tray_set_sync_enabled(tray.pin_mut(), enabled);
            ffi::native_tray_set_tooltip(tray.pin_mut(), &QString::from(tooltip));
        }
    }
}

fn to_qvariant_list(libraries: &[UiLibrary]) -> QVariant {
    let mut list = QVariantList::default();
    list.reserve(libraries.len().try_into().unwrap_or(isize::MAX));
    for library in libraries {
        let mut map = QVariantMap::default();
        insert_string(&mut map, "libraryId", &library.library_id);
        insert_string(&mut map, "label", &library.label);
        insert_string(&mut map, "runtimeCode", library.runtime_code);
        insert_string(&mut map, "runtimeLabel", library.runtime_label);
        insert_string(&mut map, "rootCode", library.root_code);
        insert_string(&mut map, "rootLabel", library.root_label);
        insert_string(&mut map, "authCode", library.auth_code);
        insert_string(&mut map, "authLabel", library.auth_label);
        insert_string(&mut map, "conflictCode", library.conflict_code);
        insert_string(&mut map, "conflictLabel", library.conflict_label);
        insert_string(&mut map, "outcomeCode", library.outcome_code.unwrap_or(""));
        insert_string(&mut map, "outcomeLabel", library.outcome_label);
        insert_bool(&mut map, "hasNextDue", library.next_due_ms.is_some());
        insert_u64(
            &mut map,
            "nextDueMs",
            library.next_due_ms.unwrap_or_default(),
        );
        insert_bool(&mut map, "wakePending", library.wake_pending);
        insert_u32(&mut map, "transientFailures", library.transient_failures);
        insert_bool(&mut map, "needsAttention", library.needs_attention);
        insert_bool(&mut map, "canSync", library.can_sync);
        list.append(QVariant::from(&map));
    }
    QVariant::from(&list)
}

fn insert_string(map: &mut QVariantMap, key: &str, value: &str) {
    let key = QString::from(key);
    let value = QVariant::from(&QString::from(value));
    map.insert_clone(&key, &value);
}

fn insert_bool(map: &mut QVariantMap, key: &str, value: bool) {
    let key = QString::from(key);
    let value = QVariant::from(&value);
    map.insert_clone(&key, &value);
}

fn insert_u32(map: &mut QVariantMap, key: &str, value: u32) {
    let key = QString::from(key);
    let value = QVariant::from(&value);
    map.insert_clone(&key, &value);
}

fn insert_u64(map: &mut QVariantMap, key: &str, value: u64) {
    let key = QString::from(key);
    let value = QVariant::from(&value);
    map.insert_clone(&key, &value);
}

fn safe_i32(value: usize) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

// Keep the async runtime handle type visible to rustc's generated bridge code.
#[allow(dead_code)]
fn _runtime_handle_is_send(handle: &Handle) -> bool {
    let _ = handle;
    true
}
