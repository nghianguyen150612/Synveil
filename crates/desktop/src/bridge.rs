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
    AutostartState, BackgroundClientManager, BackgroundLaunchError, BackgroundLaunchResult,
    DesktopController, DesktopControllerCommandResult, DesktopControllerConfig,
    DesktopControllerConflictAction, DesktopControllerConflictResolutionRequest, LibraryId,
    ServerProfileId,
};
use tokio::runtime::{Builder, Handle, Runtime};
use zeroize::Zeroizing;

use crate::{actions, presentation, profile};
use actions::{LatestValue, StartupIntentGate, SyncRequestGate};
use presentation::{UiAttentionItem, UiLibrary, UiRecoveryItem, UiSnapshot};

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
        fn native_desktop_settings_load_close_to_tray() -> bool;
        fn native_desktop_settings_save_close_to_tray(enabled: bool) -> bool;
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
        #[qproperty(bool, profile_configured)]
        #[qproperty(bool, profile_authenticated)]
        #[qproperty(QString, profile_display_name)]
        #[qproperty(QString, profile_server_url)]
        #[qproperty(bool, configuration_busy)]
        #[qproperty(QString, configuration_feedback)]
        #[qproperty(bool, library_setup_required)]
        #[qproperty(bool, library_setup_busy)]
        #[qproperty(QString, library_setup_feedback)]
        #[qproperty(QString, library_setup_folder)]
        #[qproperty(QString, last_error_label)]
        #[qproperty(i32, recovery_action_required_count)]
        #[qproperty(i32, recovery_waiting_count)]
        #[qproperty(QString, client_recovery_label)]
        #[qproperty(QVariant, recovery_items)]
        #[qproperty(bool, recovery_items_truncated)]
        #[qproperty(bool, recovery_busy)]
        #[qproperty(QString, recovery_feedback)]
        #[qproperty(bool, credential_store_unavailable)]
        #[qproperty(QVariant, libraries)]
        #[qproperty(i32, library_count)]
        #[qproperty(i32, attention_count)]
        #[qproperty(i32, conflict_attention_count)]
        #[qproperty(i32, other_attention_count)]
        #[qproperty(QVariant, attention_items)]
        #[qproperty(bool, attention_items_truncated)]
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
        #[qproperty(bool, selected_can_sign_out)]
        #[qproperty(bool, selected_needs_attention)]
        #[qproperty(bool, sync_in_flight)]
        #[qproperty(QString, sync_feedback)]
        #[qproperty(QString, selected_attention_id)]
        #[qproperty(QString, selected_attention_library_label)]
        #[qproperty(QString, selected_attention_category_label)]
        #[qproperty(QString, selected_attention_path)]
        #[qproperty(QString, selected_attention_kind_label)]
        #[qproperty(QString, selected_attention_detail)]
        #[qproperty(bool, selected_attention_can_accept_remote)]
        #[qproperty(bool, selected_attention_can_retry_local)]
        #[qproperty(bool, attention_resolution_busy)]
        #[qproperty(QString, attention_feedback)]
        #[qproperty(bool, sync_paused)]
        #[qproperty(QString, sync_control_label)]
        #[qproperty(bool, sync_control_busy)]
        #[qproperty(QString, sync_control_feedback)]
        #[qproperty(QString, background_startup_state)]
        #[qproperty(bool, background_startup_busy)]
        #[qproperty(QString, background_startup_feedback)]
        #[qproperty(bool, close_to_tray)]
        #[qproperty(QString, close_to_tray_feedback)]
        #[qproperty(bool, auth_in_flight)]
        #[qproperty(QString, auth_feedback)]
        #[qproperty(bool, auth_required)]
        #[qproperty(bool, auth_status_unknown)]
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

        #[cxx_name = "selectAttention"]
        #[qinvokable]
        fn select_attention(self: Pin<&mut Self>, attention_id: QString);

        #[cxx_name = "acceptSelectedConflict"]
        #[qinvokable]
        fn accept_selected_conflict(self: Pin<&mut Self>);

        #[cxx_name = "retrySelectedConflict"]
        #[qinvokable]
        fn retry_selected_conflict(self: Pin<&mut Self>);

        #[cxx_name = "syncNow"]
        #[qinvokable]
        fn sync_now(self: Pin<&mut Self>);

        #[cxx_name = "retrySelectedRecovery"]
        #[qinvokable]
        fn retry_selected_recovery(
            self: Pin<&mut Self>,
            library_id: QString,
            connection_generation: i32,
        );

        #[cxx_name = "startBackgroundClient"]
        #[qinvokable]
        fn start_background_client(self: Pin<&mut Self>);

        #[cxx_name = "resumePendingSetup"]
        #[qinvokable]
        fn resume_pending_setup(self: Pin<&mut Self>);

        #[cxx_name = "pauseSync"]
        #[qinvokable]
        fn pause_sync(self: Pin<&mut Self>);

        #[cxx_name = "resumeSync"]
        #[qinvokable]
        fn resume_sync(self: Pin<&mut Self>);

        #[cxx_name = "setBackgroundStartup"]
        #[qinvokable]
        fn set_background_startup(self: Pin<&mut Self>, enabled: bool);

        #[cxx_name = "setCloseToTray"]
        #[qinvokable]
        fn set_close_to_tray_preference(self: Pin<&mut Self>, enabled: bool);

        #[cxx_name = "syncLibrary"]
        #[qinvokable]
        fn sync_library(self: Pin<&mut Self>, library_id: QString);

        #[cxx_name = "configureProfile"]
        #[qinvokable]
        fn configure_profile(self: Pin<&mut Self>, server_url: QString, display_label: QString);

        #[cxx_name = "setLibraryFolder"]
        #[qinvokable]
        fn set_library_folder(self: Pin<&mut Self>, folder_url: QString);

        #[cxx_name = "setupLibrary"]
        #[qinvokable]
        fn setup_library(self: Pin<&mut Self>, name: QString);

        #[cxx_name = "authenticate"]
        #[qinvokable]
        fn authenticate(self: Pin<&mut Self>, enrollment_token: QString);

        #[cxx_name = "signOut"]
        #[qinvokable]
        fn sign_out(self: Pin<&mut Self>);

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
    pub(crate) profile_configured: bool,
    pub(crate) profile_authenticated: bool,
    pub(crate) profile_display_name: QString,
    pub(crate) profile_server_url: QString,
    pub(crate) configuration_busy: bool,
    pub(crate) configuration_feedback: QString,
    pub(crate) library_setup_required: bool,
    pub(crate) library_setup_busy: bool,
    pub(crate) library_setup_feedback: QString,
    pub(crate) library_setup_folder: QString,
    pub(crate) last_error_label: QString,
    pub(crate) recovery_action_required_count: i32,
    pub(crate) recovery_waiting_count: i32,
    pub(crate) client_recovery_label: QString,
    pub(crate) recovery_items: QVariant,
    pub(crate) recovery_items_truncated: bool,
    pub(crate) recovery_busy: bool,
    pub(crate) recovery_feedback: QString,
    pub(crate) credential_store_unavailable: bool,
    pub(crate) libraries: QVariant,
    pub(crate) library_count: i32,
    pub(crate) attention_count: i32,
    pub(crate) conflict_attention_count: i32,
    pub(crate) other_attention_count: i32,
    pub(crate) attention_items: QVariant,
    pub(crate) attention_items_truncated: bool,
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
    pub(crate) selected_can_sign_out: bool,
    pub(crate) selected_needs_attention: bool,
    pub(crate) sync_in_flight: bool,
    pub(crate) sync_feedback: QString,
    pub(crate) selected_attention_id: QString,
    pub(crate) selected_attention_library_label: QString,
    pub(crate) selected_attention_category_label: QString,
    pub(crate) selected_attention_path: QString,
    pub(crate) selected_attention_kind_label: QString,
    pub(crate) selected_attention_detail: QString,
    pub(crate) selected_attention_can_accept_remote: bool,
    pub(crate) selected_attention_can_retry_local: bool,
    pub(crate) attention_resolution_busy: bool,
    pub(crate) attention_feedback: QString,
    pub(crate) sync_paused: bool,
    pub(crate) sync_control_label: QString,
    pub(crate) sync_control_busy: bool,
    pub(crate) sync_control_feedback: QString,
    pub(crate) background_startup_state: QString,
    pub(crate) background_startup_busy: bool,
    pub(crate) background_startup_feedback: QString,
    pub(crate) close_to_tray: bool,
    pub(crate) close_to_tray_feedback: QString,
    pub(crate) auth_in_flight: bool,
    pub(crate) auth_feedback: QString,
    pub(crate) auth_required: bool,
    pub(crate) auth_status_unknown: bool,
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
    sync_control_gate: Arc<StartupIntentGate>,
    startup_gate: Arc<StartupIntentGate>,
    auth_gate: Arc<SyncRequestGate>,
    configuration_gate: Arc<SyncRequestGate>,
    library_setup_gate: Arc<SyncRequestGate>,
    attention_gate: Arc<SyncRequestGate>,
    recovery_gate: Arc<SyncRequestGate>,
    library_setup_root: Option<std::path::PathBuf>,
    presented: UiSnapshot,
    selected_id: Option<String>,
    selected_attention_id_value: Option<String>,
    auth_authenticated: bool,
    auth_status_initialized: bool,
    auth_delivery_unknown: bool,
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
                "Configure a Synveil server to continue."
            } else {
                "Desktop profile identity is unavailable."
            }),
            profile_configured: false,
            profile_authenticated: false,
            profile_display_name: QString::default(),
            profile_server_url: QString::default(),
            configuration_busy: false,
            configuration_feedback: QString::default(),
            library_setup_required: false,
            library_setup_busy: false,
            library_setup_feedback: QString::default(),
            library_setup_folder: QString::default(),
            last_error_label: QString::default(),
            recovery_action_required_count: 0,
            recovery_waiting_count: 0,
            client_recovery_label: QString::from("Background client status unknown"),
            recovery_items: QVariant::default(),
            recovery_items_truncated: false,
            recovery_busy: false,
            recovery_feedback: QString::default(),
            credential_store_unavailable: false,
            libraries: QVariant::default(),
            library_count: 0,
            attention_count: 0,
            conflict_attention_count: 0,
            other_attention_count: 0,
            attention_items: QVariant::default(),
            attention_items_truncated: false,
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
            selected_can_sign_out: false,
            selected_needs_attention: false,
            sync_in_flight: false,
            sync_feedback: QString::default(),
            selected_attention_id: QString::default(),
            selected_attention_library_label: QString::default(),
            selected_attention_category_label: QString::default(),
            selected_attention_path: QString::default(),
            selected_attention_kind_label: QString::default(),
            selected_attention_detail: QString::default(),
            selected_attention_can_accept_remote: false,
            selected_attention_can_retry_local: false,
            attention_resolution_busy: false,
            attention_feedback: QString::default(),
            sync_paused: false,
            sync_control_label: QString::from("Sync status unavailable"),
            sync_control_busy: false,
            sync_control_feedback: QString::default(),
            background_startup_state: QString::from("Unknown"),
            background_startup_busy: false,
            background_startup_feedback: QString::default(),
            close_to_tray: true,
            close_to_tray_feedback: QString::default(),
            auth_in_flight: false,
            auth_feedback: QString::default(),
            auth_required: false,
            auth_status_unknown: false,
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
            sync_control_gate: Arc::new(StartupIntentGate::default()),
            startup_gate: Arc::new(StartupIntentGate::default()),
            auth_gate: Arc::new(SyncRequestGate::default()),
            configuration_gate: Arc::new(SyncRequestGate::default()),
            library_setup_gate: Arc::new(SyncRequestGate::default()),
            attention_gate: Arc::new(SyncRequestGate::default()),
            recovery_gate: Arc::new(SyncRequestGate::default()),
            library_setup_root: None,
            presented,
            selected_id: None,
            selected_attention_id_value: None,
            auth_authenticated: false,
            auth_status_initialized: false,
            auth_delivery_unknown: false,
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
        let (started, runtime, controller, launch_manager) = {
            let state = self.rust();
            (
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
            let startup = launch_manager.autostart_status().await;
            let (startup_state, startup_feedback) = autostart_presentation(startup);
            let _ = dispatcher.qt_thread.queue(move |mut object| {
                object
                    .as_mut()
                    .set_background_startup_state(QString::from(startup_state));
                object
                    .as_mut()
                    .set_background_startup_feedback(QString::from(startup_feedback));
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
        let close_to_tray = ffi::native_desktop_settings_load_close_to_tray();
        self.as_mut().set_close_to_tray(close_to_tray);
        let bridge: &cxx_qt::QObject = self.as_ref().get_ref().upcast();
        let tray = ffi::native_tray_new(bridge);
        if tray.is_null() {
            self.as_mut().set_tray_available(false);
            return;
        }
        let available = ffi::native_tray_is_available(&tray);
        debug_assert_eq!(actions::TRAY_ACTION_LABELS.len(), 3);
        self.as_mut().rust_mut().get_mut().tray = Some(tray);
        self.as_mut().set_close_to_tray(close_to_tray);
        self.as_mut().set_tray_available(available);
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

    fn select_attention(mut self: Pin<&mut Self>, attention_id: QString) {
        let attention_id = String::from(attention_id);
        let is_known = self
            .rust()
            .presented
            .attention_items
            .iter()
            .any(|item| item.attention_id == attention_id);
        if !is_known {
            return;
        }
        self.as_mut()
            .rust_mut()
            .get_mut()
            .selected_attention_id_value = Some(attention_id);
        update_selected_attention_properties(self);
    }

    fn accept_selected_conflict(self: Pin<&mut Self>) {
        request_attention_resolution(self, DesktopControllerConflictAction::AcceptRemote);
    }

    fn retry_selected_conflict(self: Pin<&mut Self>) {
        request_attention_resolution(
            self,
            DesktopControllerConflictAction::RetryLocalAgainstCurrentBase,
        );
    }

    fn sync_now(self: Pin<&mut Self>) {
        request_sync(self);
    }

    fn retry_selected_recovery(
        self: Pin<&mut Self>,
        library_id: QString,
        connection_generation: i32,
    ) {
        request_recovery_retry(
            self,
            Some(String::from(library_id)),
            u64::try_from(connection_generation).ok(),
        );
    }

    fn start_background_client(self: Pin<&mut Self>) {
        request_background_client_start(self);
    }

    fn resume_pending_setup(mut self: Pin<&mut Self>) {
        self.as_mut().set_library_setup_required(true);
        self.as_mut().set_library_setup_feedback(QString::from(
            "Choose the same folder and submit its library name to resume safely.",
        ));
        self.as_mut().set_recovery_feedback(QString::from(
            "Use the library setup form below; the existing pending identity will be reconciled.",
        ));
    }

    fn pause_sync(self: Pin<&mut Self>) {
        request_sync_control(self, true);
    }

    fn resume_sync(self: Pin<&mut Self>) {
        request_sync_control(self, false);
    }

    fn set_background_startup(self: Pin<&mut Self>, enabled: bool) {
        request_background_startup(self, enabled);
    }

    fn set_close_to_tray_preference(mut self: Pin<&mut Self>, enabled: bool) {
        if ffi::native_desktop_settings_save_close_to_tray(enabled) {
            self.as_mut().set_close_to_tray(enabled);
            self.as_mut()
                .set_close_to_tray_feedback(QString::from("Saved on this desktop."));
        } else {
            self.as_mut().set_close_to_tray_feedback(QString::from(
                "This desktop setting could not be saved.",
            ));
        }
    }

    fn sync_library(self: Pin<&mut Self>, library_id: QString) {
        request_sync_for(self, Some(String::from(library_id)));
    }

    fn configure_profile(self: Pin<&mut Self>, server_url: QString, display_label: QString) {
        request_profile_configuration(self, server_url, display_label);
    }

    fn set_library_folder(mut self: Pin<&mut Self>, folder_url: QString) {
        let value = String::from(folder_url);
        let path = if value.starts_with("file:") {
            url::Url::parse(&value)
                .ok()
                .and_then(|url| url.to_file_path().ok())
        } else if value.is_empty() || value.chars().any(char::is_control) {
            None
        } else {
            Some(std::path::PathBuf::from(value))
        };
        let Some(path) = path else {
            self.as_mut().rust_mut().get_mut().library_setup_root = None;
            self.as_mut()
                .set_library_setup_folder(QString::from("No folder selected"));
            return;
        };
        let Some(display) = path.to_str().map(str::to_owned) else {
            self.as_mut().rust_mut().get_mut().library_setup_root = None;
            self.as_mut()
                .set_library_setup_folder(QString::from("Selected folder is unavailable."));
            return;
        };
        self.as_mut().rust_mut().get_mut().library_setup_root = Some(path);
        self.as_mut()
            .set_library_setup_folder(QString::from(display));
    }

    fn setup_library(self: Pin<&mut Self>, name: QString) {
        request_library_setup(self, String::from(name));
    }

    /// Dispatch a transient enrollment token through the controller. The
    /// bridge does not parse, persist, log, snapshot, or otherwise retain the
    /// token; the controller owns the domain parse and the background process
    /// owns the exchange and SecretStore lifecycle.
    fn authenticate(self: Pin<&mut Self>, enrollment_token: QString) {
        request_authentication(self, Zeroizing::new(String::from(enrollment_token)));
    }

    fn sign_out(self: Pin<&mut Self>) {
        request_sign_out(self);
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

fn autostart_presentation(
    result: Result<AutostartState, BackgroundLaunchError>,
) -> (&'static str, &'static str) {
    match result {
        Ok(AutostartState::Enabled) => ("Enabled", "Startup at user login is enabled."),
        Ok(AutostartState::Disabled) => ("Disabled", "Startup at user login is disabled."),
        Ok(AutostartState::NotInstalled) => (
            "Unavailable",
            "The background client is not installed for startup.",
        ),
        Ok(AutostartState::Unavailable) => (
            "Unavailable",
            "Startup settings are unavailable on this desktop.",
        ),
        Ok(AutostartState::Unsupported) => (
            "Unavailable",
            "Startup settings are not supported on this platform.",
        ),
        Err(_) => (
            "Unavailable",
            "Startup setting status is unavailable on this desktop.",
        ),
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
    let selected_attention_id = {
        let state = object.rust();
        match state.selected_attention_id_value.as_deref() {
            Some(previous)
                if snapshot
                    .attention_items
                    .iter()
                    .any(|item| item.attention_id == previous) =>
            {
                Some(previous.to_owned())
            }
            Some(_) => None,
            None => snapshot
                .attention_items
                .first()
                .map(|item| item.attention_id.clone()),
        }
    };
    if snapshot.freshness_code == "fresh" {
        // The process-level profile state is authoritative even when the
        // server currently reports no libraries. No credential crosses this
        // boundary; only the active-enrollment boolean is projected.
        let state = object.as_mut().rust_mut().get_mut();
        state.auth_authenticated = snapshot.profile_authenticated;
        state.auth_status_initialized = snapshot.profile_configured;
        state.auth_delivery_unknown = false;
    }
    object.as_mut().rust_mut().get_mut().presented = snapshot.clone();
    object.as_mut().rust_mut().get_mut().selected_id = selected_id;
    object
        .as_mut()
        .rust_mut()
        .get_mut()
        .selected_attention_id_value = selected_attention_id;

    let profile_message = {
        let state = object.rust();
        if !state.profile_ready {
            "Desktop profile identity is unavailable."
        } else if snapshot.profile_configured {
            "Server connection configured; authenticate this device if required."
        } else {
            "Configure a Synveil server to continue."
        }
    };

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
        .set_sync_paused(snapshot.sync_control_code == "paused");
    object
        .as_mut()
        .set_sync_control_label(QString::from(snapshot.sync_control_label));
    object
        .as_mut()
        .set_profile_message(QString::from(profile_message));
    object
        .as_mut()
        .set_profile_configured(snapshot.profile_configured);
    object
        .as_mut()
        .set_profile_authenticated(snapshot.profile_authenticated);
    object.as_mut().set_profile_display_name(QString::from(
        snapshot.profile_display_name.as_deref().unwrap_or(""),
    ));
    object.as_mut().set_profile_server_url(QString::from(
        snapshot.profile_server_url.as_deref().unwrap_or(""),
    ));
    object.as_mut().set_library_setup_required(
        snapshot.freshness_code == "fresh"
            && snapshot.profile_configured
            && snapshot.profile_authenticated
            && snapshot.libraries.is_empty(),
    );
    object
        .as_mut()
        .set_last_error_label(QString::from(snapshot.last_error_label));
    object
        .as_mut()
        .set_recovery_action_required_count(safe_i32(snapshot.recovery_action_required));
    object
        .as_mut()
        .set_recovery_waiting_count(safe_i32(snapshot.recovery_waiting_count));
    object
        .as_mut()
        .set_client_recovery_label(QString::from(snapshot.client_recovery_label));
    object
        .as_mut()
        .set_recovery_items(to_qvariant_recovery_list(&snapshot.recovery_items));
    object
        .as_mut()
        .set_recovery_items_truncated(snapshot.recovery_items_truncated);
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
        .set_conflict_attention_count(safe_i32(snapshot.conflict_attention_count));
    object
        .as_mut()
        .set_other_attention_count(safe_i32(snapshot.other_attention_count));
    object
        .as_mut()
        .set_attention_items(to_qvariant_attention_list(&snapshot.attention_items));
    object
        .as_mut()
        .set_attention_items_truncated(snapshot.attention_items_truncated);
    object
        .as_mut()
        .set_root_unavailable_count(safe_i32(snapshot.root_unavailable_count));
    object
        .as_mut()
        .set_libraries_truncated(snapshot.libraries_truncated);
    update_selected_properties(object.as_mut());
    update_selected_attention_properties(object.as_mut());
    update_tray(object);
}

fn update_selected_properties(mut object: Pin<&mut ffi::DesktopUiBridge>) {
    let (
        profile_configured,
        profile_authenticated,
        auth_authenticated,
        auth_status_initialized,
        auth_delivery_unknown,
    ) = {
        let state = object.rust();
        (
            state.presented.profile_configured,
            state.presented.profile_authenticated,
            state.auth_authenticated,
            state.auth_status_initialized,
            state.auth_delivery_unknown,
        )
    };
    let effective_authenticated = profile_authenticated || auth_authenticated;
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
    let selected_auth_recovery_required = selected
        .as_ref()
        .is_some_and(|library| matches!(library.auth_code, "missing" | "blocked" | "revoked"));
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
    let selected_auth_label = selected.as_ref().map_or_else(
        || {
            if !profile_configured {
                "Configure server"
            } else if effective_authenticated {
                "Authentication ready"
            } else if auth_delivery_unknown || !auth_status_initialized {
                "Checking authentication status"
            } else {
                "Authentication required"
            }
        },
        |library| library.auth_label,
    );
    object
        .as_mut()
        .set_selected_auth_label(QString::from(selected_auth_label));
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
    object.as_mut().set_selected_can_sign_out(
        profile_configured
            && (effective_authenticated
                || selected.as_ref().is_some_and(|library| {
                    matches!(library.auth_code, "blocked" | "revoked" | "unknown")
                })),
    );
    object.as_mut().set_auth_required(
        profile_configured
            && !auth_delivery_unknown
            && (selected_auth_recovery_required
                || (auth_status_initialized && !effective_authenticated)),
    );
    object.as_mut().set_auth_status_unknown(
        auth_delivery_unknown || (profile_configured && !auth_status_initialized),
    );
    object.as_mut().set_selected_needs_attention(
        selected
            .as_ref()
            .is_some_and(|library| library.needs_attention),
    );
}

fn update_selected_attention_properties(mut object: Pin<&mut ffi::DesktopUiBridge>) {
    let selected = {
        let state = object.rust();
        state
            .selected_attention_id_value
            .as_ref()
            .and_then(|selected_id| {
                state
                    .presented
                    .attention_items
                    .iter()
                    .find(|item| &item.attention_id == selected_id)
            })
            .cloned()
    };
    let selected_id = selected
        .as_ref()
        .map_or("", |item| item.attention_id.as_str());
    object
        .as_mut()
        .set_selected_attention_id(QString::from(selected_id));
    object
        .as_mut()
        .set_selected_attention_library_label(QString::from(
            selected
                .as_ref()
                .map_or("No attention item selected", |item| {
                    item.library_label.as_str()
                }),
        ));
    object
        .as_mut()
        .set_selected_attention_category_label(QString::from(
            selected
                .as_ref()
                .map_or("No attention item selected", |item| {
                    item.category_label.as_str()
                }),
        ));
    object.as_mut().set_selected_attention_path(QString::from(
        selected
            .as_ref()
            .map_or("Item details unavailable", |item| item.path_label.as_str()),
    ));
    object
        .as_mut()
        .set_selected_attention_kind_label(QString::from(
            selected
                .as_ref()
                .map_or("Item", |item| item.item_kind_label.as_str()),
        ));
    let detail = selected.as_ref().map_or_else(
        || "Select an attention item to review its safe details.".to_owned(),
        attention_detail,
    );
    object
        .as_mut()
        .set_selected_attention_detail(QString::from(detail));
    object.as_mut().set_selected_attention_can_accept_remote(
        selected.as_ref().is_some_and(|item| item.can_accept_remote),
    );
    object.as_mut().set_selected_attention_can_retry_local(
        selected.as_ref().is_some_and(|item| item.can_retry_local),
    );
}

fn attention_detail(item: &UiAttentionItem) -> String {
    let mut detail = item.category_label.clone();
    if let Some(state) = &item.remote_observed_state {
        detail.push_str(" · Remote state: ");
        detail.push_str(state);
    }
    if let (Some(local), Some(remote)) = (item.local_length, item.remote_length) {
        detail.push_str(&format!(" · Local {local} bytes · Remote {remote} bytes"));
    }
    detail
}

fn request_attention_resolution(
    mut object: Pin<&mut ffi::DesktopUiBridge>,
    action: DesktopControllerConflictAction,
) {
    let (item, gate, controller, runtime, live_test) = {
        let state = object.rust();
        let item = state
            .selected_attention_id_value
            .as_ref()
            .and_then(|selected_id| {
                state
                    .presented
                    .attention_items
                    .iter()
                    .find(|item| &item.attention_id == selected_id)
            })
            .cloned();
        (
            item,
            Arc::clone(&state.attention_gate),
            state.controller.clone(),
            state
                .runtime
                .as_ref()
                .map(|runtime| runtime.handle().clone()),
            state.live_test,
        )
    };

    let Some(item) = item else {
        object
            .as_mut()
            .set_attention_feedback(QString::from("Select an attention item first."));
        return;
    };
    if (action == DesktopControllerConflictAction::AcceptRemote && !item.can_accept_remote)
        || (action == DesktopControllerConflictAction::RetryLocalAgainstCurrentBase
            && !item.can_retry_local)
    {
        object.as_mut().set_attention_feedback(QString::from(
            "That action is not available for this attention item.",
        ));
        return;
    }
    if !gate.try_acquire() {
        object.as_mut().set_attention_feedback(QString::from(
            "A conflict decision is already being processed.",
        ));
        return;
    }
    let Some(runtime) = runtime else {
        gate.release();
        object
            .as_mut()
            .set_attention_feedback(QString::from("Desktop shell tasks are unavailable."));
        return;
    };
    let Ok(library_id) = item.library_id.parse::<LibraryId>() else {
        gate.release();
        object
            .as_mut()
            .set_attention_feedback(QString::from("That library is no longer available."));
        return;
    };
    let Ok(conflict_id) = item.attention_id.parse() else {
        gate.release();
        object
            .as_mut()
            .set_attention_feedback(QString::from("That attention item is no longer available."));
        return;
    };
    let Ok(intent_id) = item.intent_id.parse() else {
        gate.release();
        object
            .as_mut()
            .set_attention_feedback(QString::from("That attention item is no longer available."));
        return;
    };
    let request = DesktopControllerConflictResolutionRequest {
        library_id,
        conflict_id,
        intent_id,
        detected_at_ms: item.detected_at_ms,
        action,
    };
    object.as_mut().set_attention_resolution_busy(true);
    object
        .as_mut()
        .set_attention_feedback(QString::from(match action {
            DesktopControllerConflictAction::AcceptRemote => "Saving remote decision…",
            DesktopControllerConflictAction::RetryLocalAgainstCurrentBase => {
                "Saving local retry decision…"
            }
        }));
    if live_test {
        eprintln!(
            "SYNVEIL-LIVE-UI ACTION conflict_resolution={}",
            match action {
                DesktopControllerConflictAction::AcceptRemote => "accept_remote",
                DesktopControllerConflictAction::RetryLocalAgainstCurrentBase => "retry_local",
            }
        );
    }
    let qt_thread = object.as_ref().get_ref().qt_thread();
    runtime.spawn(async move {
        let (feedback, unknown) = match controller.resolve_conflict(request).await {
            Ok(result) => {
                let unknown = result == DesktopControllerCommandResult::OutcomeUnknown;
                if unknown {
                    controller.refresh_state();
                }
                (presentation::command_feedback(result), unknown)
            }
            Err(_) => ("Background service unavailable.", false),
        };
        if live_test {
            eprintln!("SYNVEIL-LIVE-UI RESULT conflict_feedback={feedback}");
        }
        gate.release();
        let _ = qt_thread.queue(move |mut object| {
            object.as_mut().set_attention_resolution_busy(false);
            object
                .as_mut()
                .set_attention_feedback(QString::from(feedback));
            if unknown {
                object.as_mut().set_attention_feedback(QString::from(
                    "Request status is unknown; refreshed attention status without replaying.",
                ));
            }
            update_selected_attention_properties(object);
        });
    });
}

fn request_sync(object: Pin<&mut ffi::DesktopUiBridge>) {
    request_sync_for(object, None);
}

fn request_recovery_retry(
    mut object: Pin<&mut ffi::DesktopUiBridge>,
    requested_library_id: Option<String>,
    requested_generation: Option<u64>,
) {
    let (item, gate, controller, runtime, live_test, generation) = {
        let state = object.rust();
        let selected_id = requested_library_id
            .as_deref()
            .or(state.selected_id.as_deref());
        let item = state
            .presented
            .recovery_items
            .iter()
            .filter(|item| item.action_code == Some("check_again"))
            .find(|item| {
                selected_id
                    .is_some_and(|selected_id| item.library_id.as_deref() == Some(selected_id))
            })
            .or_else(|| {
                state
                    .presented
                    .recovery_items
                    .iter()
                    .find(|item| item.action_code == Some("check_again"))
            })
            .cloned();
        (
            item,
            Arc::clone(&state.recovery_gate),
            state.controller.clone(),
            state
                .runtime
                .as_ref()
                .map(|runtime| runtime.handle().clone()),
            state.live_test,
            state.presented.connection_generation,
        )
    };

    let Some(item) = item else {
        object.as_mut().set_recovery_feedback(QString::from(
            "There is no retryable recovery item in the current status.",
        ));
        return;
    };
    if requested_generation != Some(item.connection_generation)
        || item.connection_generation != generation
        || requested_library_id.as_deref() != item.library_id.as_deref()
    {
        controller.refresh_state();
        object.as_mut().set_recovery_feedback(QString::from(
            "Recovery status changed; checking the current state.",
        ));
        return;
    }
    let Some(library_id) = item.library_id.as_deref().and_then(|id| id.parse().ok()) else {
        controller.refresh_state();
        object.as_mut().set_recovery_feedback(QString::from(
            "Recovery identity changed; checking the current state.",
        ));
        return;
    };
    if !gate.try_acquire() {
        object
            .as_mut()
            .set_recovery_feedback(QString::from("A recovery check is already in progress."));
        return;
    }
    let Some(runtime) = runtime else {
        gate.release();
        object
            .as_mut()
            .set_recovery_feedback(QString::from("Desktop shell tasks are unavailable."));
        return;
    };

    object.as_mut().set_recovery_busy(true);
    object
        .as_mut()
        .set_recovery_feedback(QString::from("Checking the current library state…"));
    if live_test {
        eprintln!("SYNVEIL-LIVE-UI ACTION recovery_check");
    }
    let qt_thread = object.as_ref().get_ref().qt_thread();
    runtime.spawn(async move {
        let (feedback, outcome_unknown) = match controller.retry_recovery(library_id).await {
            Ok(result) => {
                let unknown = result == DesktopControllerCommandResult::OutcomeUnknown;
                if unknown {
                    // The wake may already have been admitted. Refresh only;
                    // never replay it after a lost response.
                    controller.refresh_state();
                }
                let feedback = if unknown {
                    "Recovery check status is unknown; refreshed without replaying."
                } else {
                    presentation::command_feedback(result)
                };
                (feedback, unknown)
            }
            Err(_) => ("Background service unavailable.", false),
        };
        if live_test {
            eprintln!("SYNVEIL-LIVE-UI RESULT recovery_feedback={feedback}");
        }
        gate.release();
        let _ = qt_thread.queue(move |mut object| {
            object.as_mut().set_recovery_busy(false);
            object
                .as_mut()
                .set_recovery_feedback(QString::from(feedback));
            if outcome_unknown {
                object.as_mut().set_last_error_label(QString::from(
                    "Request status is unknown; current status is being refreshed.",
                ));
            }
        });
    });
}

fn request_background_client_start(mut object: Pin<&mut ffi::DesktopUiBridge>) {
    let (gate, manager, controller, runtime, live_test) = {
        let state = object.rust();
        (
            Arc::clone(&state.recovery_gate),
            state.launch_manager.clone(),
            state.controller.clone(),
            state
                .runtime
                .as_ref()
                .map(|runtime| runtime.handle().clone()),
            state.live_test,
        )
    };
    if !gate.try_acquire() {
        object.as_mut().set_recovery_feedback(QString::from(
            "Background client recovery is already in progress.",
        ));
        return;
    }
    let Some(runtime) = runtime else {
        gate.release();
        object
            .as_mut()
            .set_recovery_feedback(QString::from("Desktop shell tasks are unavailable."));
        return;
    };
    object.as_mut().set_recovery_busy(true);
    object
        .as_mut()
        .set_recovery_feedback(QString::from("Starting the existing background client…"));
    if live_test {
        eprintln!("SYNVEIL-LIVE-UI ACTION start_background_client");
    }
    let qt_thread = object.as_ref().get_ref().qt_thread();
    runtime.spawn(async move {
        let result = manager.ensure_running().await;
        let feedback = launch_result_label(result);
        controller.refresh_state();
        if live_test {
            eprintln!("SYNVEIL-LIVE-UI RESULT start_background_client={feedback}");
        }
        gate.release();
        let _ = qt_thread.queue(move |mut object| {
            object.as_mut().set_recovery_busy(false);
            object
                .as_mut()
                .set_recovery_feedback(QString::from(feedback));
            object.as_mut().set_launch_label(QString::from(feedback));
        });
    });
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
    let (library_id, can_sync, paused, gate, controller, runtime, live_test) = {
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
            state.presented.sync_control_code == "paused",
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
            } else if paused {
                "Sync is paused. Resume Sync in Settings to continue."
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

fn request_sync_control(mut object: Pin<&mut ffi::DesktopUiBridge>, paused: bool) {
    let (gate, controller, runtime, live_test) = {
        let state = object.rust();
        (
            Arc::clone(&state.sync_control_gate),
            state.controller.clone(),
            state
                .runtime
                .as_ref()
                .map(|runtime| runtime.handle().clone()),
            state.live_test,
        )
    };
    gate.set_intent(paused);
    let Some((mut target, mut generation)) = gate.try_start() else {
        object.as_mut().set_sync_control_busy(true);
        object
            .as_mut()
            .set_sync_control_feedback(QString::from("Applying the latest sync preference…"));
        return;
    };
    let Some(runtime) = runtime else {
        gate.release();
        object
            .as_mut()
            .set_sync_control_feedback(QString::from("Desktop shell tasks are unavailable."));
        return;
    };

    object.as_mut().set_sync_control_busy(true);
    object
        .as_mut()
        .set_sync_control_feedback(QString::from(if paused {
            "Pausing sync…"
        } else {
            "Resuming sync…"
        }));
    if live_test {
        eprintln!(
            "SYNVEIL-LIVE-UI ACTION {}",
            if paused { "pause_sync" } else { "resume_sync" }
        );
    }
    let qt_thread = object.as_ref().get_ref().qt_thread();
    runtime.spawn(async move {
        let (feedback, applied, outcome_unknown) = loop {
            let result = if target {
                controller.pause_sync().await
            } else {
                controller.resume_sync().await
            };
            let outcome = match result {
                Ok(result) => {
                    let applied = if target {
                        matches!(
                            result,
                            DesktopControllerCommandResult::Paused
                                | DesktopControllerCommandResult::AlreadyPaused
                        )
                    } else {
                        matches!(
                            result,
                            DesktopControllerCommandResult::Resumed
                                | DesktopControllerCommandResult::AlreadyRunning
                        )
                    };
                    let outcome_unknown = result == DesktopControllerCommandResult::OutcomeUnknown;
                    if outcome_unknown {
                        controller.refresh_state();
                    }
                    (
                        presentation::sync_control_feedback(result),
                        applied,
                        outcome_unknown,
                    )
                }
                Err(_) => ("Background service unavailable.", false, false),
            };
            let (latest_target, latest_generation) = gate.current();
            if latest_generation != generation {
                target = latest_target;
                generation = latest_generation;
                continue;
            }
            break outcome;
        };
        if live_test {
            eprintln!("SYNVEIL-LIVE-UI RESULT sync_control_feedback={feedback}");
        }
        gate.release();
        let _ = qt_thread.queue(move |mut object| {
            let (_, current_generation) = gate.current();
            if current_generation != generation {
                return;
            }
            object.as_mut().set_sync_control_busy(false);
            object
                .as_mut()
                .set_sync_control_feedback(QString::from(feedback));
            if applied && !outcome_unknown {
                object.as_mut().set_sync_paused(target);
                object
                    .as_mut()
                    .set_sync_control_label(QString::from(if target {
                        "Paused by user"
                    } else {
                        "Running"
                    }));
            }
            update_tray(object);
        });
    });
}

fn request_background_startup(mut object: Pin<&mut ffi::DesktopUiBridge>, enabled: bool) {
    let (gate, manager, runtime) = {
        let state = object.rust();
        (
            Arc::clone(&state.startup_gate),
            state.launch_manager.clone(),
            state
                .runtime
                .as_ref()
                .map(|runtime| runtime.handle().clone()),
        )
    };
    gate.set_intent(enabled);
    let Some((mut target, mut generation)) = gate.try_start() else {
        object.as_mut().set_background_startup_busy(true);
        object
            .as_mut()
            .set_background_startup_feedback(QString::from(
                "Applying the latest startup preference…",
            ));
        return;
    };
    let Some(runtime) = runtime else {
        gate.release();
        object
            .as_mut()
            .set_background_startup_feedback(QString::from("Desktop shell tasks are unavailable."));
        return;
    };

    object.as_mut().set_background_startup_busy(true);
    object
        .as_mut()
        .set_background_startup_feedback(QString::from("Updating startup preference…"));
    let qt_thread = object.as_ref().get_ref().qt_thread();
    runtime.spawn(async move {
        let (state, feedback) = loop {
            let operation = if target {
                manager.enable_autostart().await
            } else {
                manager.disable_autostart().await
            };
            let presentation = match manager.autostart_status().await {
                Ok(state) => autostart_presentation(Ok(state)),
                Err(_) => autostart_presentation(operation),
            };
            let (latest_target, latest_generation) = gate.current();
            if latest_generation != generation {
                target = latest_target;
                generation = latest_generation;
                continue;
            }
            break presentation;
        };
        gate.release();
        let _ = qt_thread.queue(move |mut object| {
            let (_, current_generation) = gate.current();
            if current_generation != generation {
                return;
            }
            object.as_mut().set_background_startup_busy(false);
            object
                .as_mut()
                .set_background_startup_state(QString::from(state));
            object
                .as_mut()
                .set_background_startup_feedback(QString::from(feedback));
        });
    });
}

fn request_profile_configuration(
    mut object: Pin<&mut ffi::DesktopUiBridge>,
    server_url: QString,
    display_label: QString,
) {
    let server_url = String::from(server_url).trim().to_owned();
    let display_label = {
        let display_label = String::from(display_label).trim().to_owned();
        if display_label.is_empty() {
            "Synveil server".to_owned()
        } else {
            display_label
        }
    };
    let (profile_ready, gate, controller, runtime, live_test) = {
        let state = object.rust();
        (
            state.profile_ready,
            Arc::clone(&state.configuration_gate),
            state.controller.clone(),
            state
                .runtime
                .as_ref()
                .map(|runtime| runtime.handle().clone()),
            state.live_test,
        )
    };

    if !profile_ready {
        object
            .as_mut()
            .set_configuration_feedback(QString::from("Desktop profile identity is unavailable."));
        return;
    }
    if !gate.try_acquire() {
        object.as_mut().set_configuration_feedback(QString::from(
            "A server configuration request is already being processed.",
        ));
        return;
    }
    let Some(runtime) = runtime else {
        gate.release();
        object
            .as_mut()
            .set_configuration_feedback(QString::from("Desktop shell tasks are unavailable."));
        return;
    };

    object.as_mut().set_configuration_busy(true);
    object
        .as_mut()
        .set_configuration_feedback(QString::from("Checking server connection…"));
    if live_test {
        eprintln!("SYNVEIL-LIVE-UI ACTION configure_profile");
    }
    let qt_thread = object.as_ref().get_ref().qt_thread();
    runtime.spawn(async move {
        let (feedback, configured, outcome_unknown) = match controller
            .configure_profile(server_url, display_label)
            .await
        {
            Ok(result) => {
                let outcome_unknown = result == DesktopControllerCommandResult::OutcomeUnknown;
                if outcome_unknown {
                    controller.refresh_state();
                }
                (
                    presentation::command_feedback(result),
                    matches!(
                        result,
                        DesktopControllerCommandResult::ProfileConfigured
                            | DesktopControllerCommandResult::ProfileAlreadyConfigured
                    ),
                    outcome_unknown,
                )
            }
            Err(_) => ("Background service unavailable.", false, false),
        };
        if live_test {
            eprintln!("SYNVEIL-LIVE-UI RESULT profile_feedback={feedback}");
        }
        gate.release();
        let _ = qt_thread.queue(move |mut object| {
            object.as_mut().set_configuration_busy(false);
            object
                .as_mut()
                .set_configuration_feedback(QString::from(feedback));
            if configured {
                object.as_mut().set_profile_configured(true);
            }
            if outcome_unknown {
                object
                    .as_mut()
                    .set_profile_message(QString::from("Server status is unknown; refreshing."));
            }
        });
    });
}

fn request_library_setup(mut object: Pin<&mut ffi::DesktopUiBridge>, name: String) {
    let name = name.trim().to_owned();
    let (required, root, gate, controller, runtime, live_test) = {
        let state = object.rust();
        (
            state.library_setup_required,
            state.library_setup_root.clone(),
            Arc::clone(&state.library_setup_gate),
            state.controller.clone(),
            state
                .runtime
                .as_ref()
                .map(|runtime| runtime.handle().clone()),
            state.live_test,
        )
    };
    if !required {
        object.as_mut().set_library_setup_feedback(QString::from(
            "Library setup becomes available after this device is authenticated.",
        ));
        return;
    }
    if name.is_empty() {
        object
            .as_mut()
            .set_library_setup_feedback(QString::from("Enter a library name."));
        return;
    }
    if name.len() > 1_024 || name.chars().any(char::is_control) {
        object
            .as_mut()
            .set_library_setup_feedback(QString::from("Choose a shorter library name."));
        return;
    }
    let Some(root) = root else {
        object
            .as_mut()
            .set_library_setup_feedback(QString::from("Choose a local folder first."));
        return;
    };
    let Some(root_path) = root.to_str().map(str::to_owned) else {
        object
            .as_mut()
            .set_library_setup_feedback(QString::from("The selected folder is unavailable."));
        return;
    };
    if !gate.try_acquire() {
        object.as_mut().set_library_setup_feedback(QString::from(
            "A library setup request is already being processed.",
        ));
        return;
    }
    let Some(runtime) = runtime else {
        gate.release();
        object
            .as_mut()
            .set_library_setup_feedback(QString::from("Desktop shell tasks are unavailable."));
        return;
    };
    object.as_mut().set_library_setup_busy(true);
    object
        .as_mut()
        .set_library_setup_feedback(QString::from("Creating library…"));
    if live_test {
        eprintln!("SYNVEIL-LIVE-UI ACTION setup_library");
    }
    let qt_thread = object.as_ref().get_ref().qt_thread();
    runtime.spawn(async move {
        let result = controller.setup_library(name, root_path).await;
        let (feedback, configured) = match result {
            Ok(result) => {
                controller.refresh_state();
                (
                    presentation::library_setup_feedback(result),
                    matches!(
                        result,
                        DesktopControllerCommandResult::LibraryConfigured
                            | DesktopControllerCommandResult::LibraryAlreadyConfigured
                    ),
                )
            }
            Err(_) => ("Background service unavailable.", false),
        };
        if live_test {
            eprintln!("SYNVEIL-LIVE-UI RESULT setup_feedback={feedback}");
        }
        gate.release();
        let _ = qt_thread.queue(move |mut object| {
            object.as_mut().set_library_setup_busy(false);
            object
                .as_mut()
                .set_library_setup_feedback(QString::from(feedback));
            if configured {
                object.as_mut().set_library_setup_required(false);
            }
        });
    });
}

fn request_authentication(mut object: Pin<&mut ffi::DesktopUiBridge>, input: Zeroizing<String>) {
    let (profile_configured, gate, controller, runtime, live_test) = {
        let state = object.rust();
        (
            state.presented.profile_configured,
            Arc::clone(&state.auth_gate),
            state.controller.clone(),
            state
                .runtime
                .as_ref()
                .map(|runtime| runtime.handle().clone()),
            state.live_test,
        )
    };

    if !profile_configured {
        object.as_mut().set_auth_feedback(QString::from(
            "Configure a Synveil server before signing in.",
        ));
        return;
    }
    if !gate.try_acquire() {
        object.as_mut().set_auth_feedback(QString::from(
            "An authentication request is already being processed.",
        ));
        return;
    }
    let Some(runtime) = runtime else {
        gate.release();
        object
            .as_mut()
            .set_auth_feedback(QString::from("Desktop shell tasks are unavailable."));
        return;
    };

    object.as_mut().set_auth_in_flight(true);
    object
        .as_mut()
        .set_auth_feedback(QString::from("Signing in…"));
    if live_test {
        eprintln!("SYNVEIL-LIVE-UI ACTION authenticate");
    }
    let qt_thread = object.as_ref().get_ref().qt_thread();
    runtime.spawn(async move {
        let (feedback, authenticated, outcome_unknown, secure_store_unavailable) =
            match controller.authenticate(input.as_str()).await {
                Ok(result) => {
                    let outcome_unknown = result == DesktopControllerCommandResult::OutcomeUnknown;
                    if outcome_unknown {
                        controller.refresh_auth_state();
                    }
                    (
                        presentation::auth_feedback(result),
                        result == DesktopControllerCommandResult::Authenticated,
                        outcome_unknown,
                        result == DesktopControllerCommandResult::SecureStoreUnavailable,
                    )
                }
                Err(_) => ("Background service unavailable.", false, false, false),
            };
        if live_test {
            eprintln!("SYNVEIL-LIVE-UI RESULT auth_feedback={feedback}");
        }
        gate.release();
        let _ = qt_thread.queue(move |mut object| {
            object.as_mut().set_auth_in_flight(false);
            object.as_mut().set_auth_feedback(QString::from(feedback));
            object
                .as_mut()
                .set_credential_store_unavailable(secure_store_unavailable);
            if authenticated {
                let state = object.as_mut().rust_mut().get_mut();
                state.auth_authenticated = true;
                state.auth_status_initialized = true;
                state.auth_delivery_unknown = false;
                object.as_mut().set_profile_authenticated(true);
            }
            if outcome_unknown {
                object.as_mut().rust_mut().get_mut().auth_delivery_unknown = true;
            }
            update_selected_properties(object);
        });
    });
}

fn request_sign_out(mut object: Pin<&mut ffi::DesktopUiBridge>) {
    let (profile_configured, gate, controller, runtime, live_test) = {
        let state = object.rust();
        (
            state.presented.profile_configured,
            Arc::clone(&state.auth_gate),
            state.controller.clone(),
            state
                .runtime
                .as_ref()
                .map(|runtime| runtime.handle().clone()),
            state.live_test,
        )
    };

    if !profile_configured {
        object
            .as_mut()
            .set_auth_feedback(QString::from("Configure a Synveil server first."));
        return;
    }
    if !gate.try_acquire() {
        object.as_mut().set_auth_feedback(QString::from(
            "An authentication request is already being processed.",
        ));
        return;
    }
    let Some(runtime) = runtime else {
        gate.release();
        object
            .as_mut()
            .set_auth_feedback(QString::from("Desktop shell tasks are unavailable."));
        return;
    };

    object.as_mut().set_auth_in_flight(true);
    object
        .as_mut()
        .set_auth_feedback(QString::from("Signing out…"));
    if live_test {
        eprintln!("SYNVEIL-LIVE-UI ACTION sign_out");
    }
    let qt_thread = object.as_ref().get_ref().qt_thread();
    runtime.spawn(async move {
        let (feedback, signed_out, outcome_unknown, secure_store_unavailable) =
            match controller.sign_out().await {
                Ok(result) => {
                    let outcome_unknown = result == DesktopControllerCommandResult::OutcomeUnknown;
                    if outcome_unknown {
                        controller.refresh_auth_state();
                    }
                    (
                        presentation::auth_feedback(result),
                        result == DesktopControllerCommandResult::SignedOut,
                        outcome_unknown,
                        result == DesktopControllerCommandResult::SecureStoreUnavailable,
                    )
                }
                Err(_) => ("Background service unavailable.", false, false, false),
            };
        if live_test {
            eprintln!("SYNVEIL-LIVE-UI RESULT auth_feedback={feedback}");
        }
        gate.release();
        let _ = qt_thread.queue(move |mut object| {
            object.as_mut().set_auth_in_flight(false);
            object.as_mut().set_auth_feedback(QString::from(feedback));
            object
                .as_mut()
                .set_credential_store_unavailable(secure_store_unavailable);
            if signed_out {
                let state = object.as_mut().rust_mut().get_mut();
                state.auth_authenticated = false;
                state.auth_status_initialized = true;
                state.auth_delivery_unknown = false;
                object.as_mut().set_profile_authenticated(false);
            }
            if outcome_unknown {
                object.as_mut().rust_mut().get_mut().auth_delivery_unknown = true;
            }
            update_selected_properties(object);
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

fn to_qvariant_recovery_list(items: &[UiRecoveryItem]) -> QVariant {
    let mut list = QVariantList::default();
    list.reserve(items.len().try_into().unwrap_or(isize::MAX));
    for item in items {
        let mut map = QVariantMap::default();
        insert_string(&mut map, "recoveryId", &item.recovery_id);
        insert_string(
            &mut map,
            "libraryId",
            item.library_id.as_deref().unwrap_or(""),
        );
        insert_string(&mut map, "libraryLabel", &item.library_label);
        insert_string(&mut map, "kindCode", item.kind_code);
        insert_string(&mut map, "kindLabel", item.kind_label);
        insert_string(&mut map, "detail", item.detail);
        insert_string(&mut map, "actionCode", item.action_code.unwrap_or(""));
        insert_string(&mut map, "actionLabel", item.action_label);
        insert_bool(&mut map, "actionRequired", item.action_required);
        insert_bool(&mut map, "waiting", item.waiting);
        insert_u64(&mut map, "connectionGeneration", item.connection_generation);
        list.append(QVariant::from(&map));
    }
    QVariant::from(&list)
}

fn to_qvariant_attention_list(items: &[UiAttentionItem]) -> QVariant {
    let mut list = QVariantList::default();
    list.reserve(items.len().try_into().unwrap_or(isize::MAX));
    for item in items {
        let mut map = QVariantMap::default();
        insert_string(&mut map, "attentionId", &item.attention_id);
        insert_string(&mut map, "libraryId", &item.library_id);
        insert_string(&mut map, "libraryLabel", &item.library_label);
        insert_string(&mut map, "categoryCode", &item.category_code);
        insert_string(&mut map, "categoryLabel", &item.category_label);
        insert_string(
            &mut map,
            "relativePath",
            item.relative_path.as_deref().unwrap_or(""),
        );
        insert_string(
            &mut map,
            "previousRelativePath",
            item.previous_relative_path.as_deref().unwrap_or(""),
        );
        insert_string(&mut map, "pathLabel", &item.path_label);
        insert_string(&mut map, "itemKindLabel", &item.item_kind_label);
        insert_bool(&mut map, "hasLocalLength", item.local_length.is_some());
        insert_u64(
            &mut map,
            "localLength",
            item.local_length.unwrap_or_default(),
        );
        insert_bool(&mut map, "hasRemoteLength", item.remote_length.is_some());
        insert_u64(
            &mut map,
            "remoteLength",
            item.remote_length.unwrap_or_default(),
        );
        insert_bool(
            &mut map,
            "hasLocalBaseRevision",
            item.local_base_revision.is_some(),
        );
        insert_u64(
            &mut map,
            "localBaseRevision",
            item.local_base_revision.unwrap_or_default(),
        );
        insert_bool(
            &mut map,
            "hasRemoteObservedRevision",
            item.remote_observed_revision.is_some(),
        );
        insert_u64(
            &mut map,
            "remoteObservedRevision",
            item.remote_observed_revision.unwrap_or_default(),
        );
        insert_string(
            &mut map,
            "remoteObservedState",
            item.remote_observed_state.as_deref().unwrap_or(""),
        );
        insert_u64(&mut map, "detectedAtMs", item.detected_at_ms);
        insert_bool(&mut map, "canAcceptRemote", item.can_accept_remote);
        insert_bool(&mut map, "canRetryLocal", item.can_retry_local);
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
