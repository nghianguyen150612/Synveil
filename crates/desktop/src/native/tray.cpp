#include "synveil-desktop/native/tray.h"

#include <QtCore/QCoreApplication>
#include <QtCore/QMetaObject>

namespace {

std::unique_ptr<QApplication> makeApplication() {
    static int argument_count = 1;
    static char application_name[] = "synveil-desktop";
    static char* arguments[] = {application_name, nullptr};
    return std::make_unique<QApplication>(argument_count, arguments);
}

} // namespace

NativeApplication::NativeApplication()
    : application_(makeApplication()) {
    QApplication::setQuitOnLastWindowClosed(false);
}

NativeApplication::~NativeApplication() = default;

void NativeApplication::setMetadata(const QString& name, const QString& version) {
    application_->setApplicationName(name);
    application_->setApplicationDisplayName(name);
    application_->setApplicationVersion(version);
    application_->setOrganizationName(QStringLiteral("Synveil"));
    application_->setOrganizationDomain(QStringLiteral("synveil.local"));
}

int NativeApplication::exec() {
    return application_->exec();
}

NativeTray::NativeTray(const QObject& bridge)
    : tray_(QApplication::style()->standardIcon(QStyle::SP_ComputerIcon)),
      menu_(),
      open_action_(QObject::tr("Open Synveil"), &menu_),
      sync_action_(QObject::tr("Sync Now"), &menu_),
      quit_action_(QObject::tr("Quit Synveil Desktop"), &menu_) {
    menu_.addAction(&open_action_);
    menu_.addAction(&sync_action_);
    menu_.addSeparator();
    menu_.addAction(&quit_action_);
    tray_.setContextMenu(&menu_);

    auto* bridge_object = const_cast<QObject*>(&bridge);
    QObject::connect(&open_action_, &QAction::triggered, bridge_object, [bridge_object] {
        QMetaObject::invokeMethod(bridge_object, "trayOpen", Qt::QueuedConnection);
    });
    QObject::connect(&sync_action_, &QAction::triggered, bridge_object, [bridge_object] {
        QMetaObject::invokeMethod(bridge_object, "traySyncNow", Qt::QueuedConnection);
    });
    QObject::connect(&quit_action_, &QAction::triggered, bridge_object, [bridge_object] {
        QMetaObject::invokeMethod(bridge_object, "trayQuit", Qt::QueuedConnection);
    });
    QObject::connect(&tray_, &QSystemTrayIcon::activated, bridge_object,
                     [bridge_object](QSystemTrayIcon::ActivationReason reason) {
                         if (reason == QSystemTrayIcon::Trigger
                             || reason == QSystemTrayIcon::DoubleClick) {
                             QMetaObject::invokeMethod(
                                 bridge_object, "trayOpen", Qt::QueuedConnection);
                         }
                     });

    if (QSystemTrayIcon::isSystemTrayAvailable()) {
        tray_.show();
    }
}

NativeTray::~NativeTray() {
    tray_.hide();
}

bool NativeTray::isAvailable() const {
    return QSystemTrayIcon::isSystemTrayAvailable();
}

void NativeTray::setSyncEnabled(bool enabled) {
    sync_action_.setEnabled(enabled);
}

void NativeTray::setTooltip(const QString& tooltip) {
    tray_.setToolTip(tooltip);
}

std::unique_ptr<NativeApplication> native_application_new() {
    return std::make_unique<NativeApplication>();
}

void native_application_set_metadata(
    NativeApplication& app,
    const QString& name,
    const QString& version) {
    app.setMetadata(name, version);
}

int native_application_exec(NativeApplication& app) {
    return app.exec();
}

bool native_qml_engine_has_root(const QQmlApplicationEngine& engine) {
    return !engine.rootObjects().isEmpty();
}

std::unique_ptr<NativeTray> native_tray_new(const QObject& bridge) {
    return std::make_unique<NativeTray>(bridge);
}

bool native_tray_is_available(const NativeTray& tray) {
    return tray.isAvailable();
}

void native_tray_set_sync_enabled(NativeTray& tray, bool enabled) {
    tray.setSyncEnabled(enabled);
}

void native_tray_set_tooltip(NativeTray& tray, const QString& tooltip) {
    tray.setTooltip(tooltip);
}
