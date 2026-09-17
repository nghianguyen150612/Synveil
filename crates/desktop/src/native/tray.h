#pragma once

#include <QtCore/qobject.h>
#include <QtCore/qlist.h>
#include <QtCore/qstring.h>
#include <QtCore/qvariant.h>
#include <QtWidgets/qaction.h>
#include <QtWidgets/qapplication.h>
#include <QtWidgets/qmenu.h>
#include <QtWidgets/qstyle.h>
#include <QtWidgets/qsystemtrayicon.h>
#include <QtQml/QQmlApplicationEngine>

#include <memory>

class NativeApplication {
public:
    NativeApplication();
    ~NativeApplication();

    void setMetadata(const QString& name, const QString& version);
    int exec();

private:
    std::unique_ptr<QApplication> application_;
};

class NativeTray {
public:
    explicit NativeTray(const QObject& bridge);
    ~NativeTray();

    bool isAvailable() const;
    void setSyncEnabled(bool enabled);
    void setTooltip(const QString& tooltip);

private:
    QSystemTrayIcon tray_;
    QMenu menu_;
    QAction open_action_;
    QAction sync_action_;
    QAction quit_action_;
};

std::unique_ptr<NativeApplication> native_application_new();
void native_application_set_metadata(
    NativeApplication& app,
    const QString& name,
    const QString& version);
int native_application_exec(NativeApplication& app);
bool native_qml_engine_has_root(const QQmlApplicationEngine& engine);

std::unique_ptr<NativeTray> native_tray_new(const QObject& bridge);
bool native_tray_is_available(const NativeTray& tray);
void native_tray_set_sync_enabled(NativeTray& tray, bool enabled);
void native_tray_set_tooltip(NativeTray& tray, const QString& tooltip);
