# plasma-face-unlock: the compiled half
#
# The Makefile is the entry point and calls this. Everything that has to be
# compiled lives here: the daemon, the agent, the client the shell code uses,
# the PAM module and the tests. The shell code, the units and the models are
# installed by the Makefile, which is where the paths come from.

cmake_minimum_required(VERSION 3.22)
project(plasma-face-unlock VERSION 1.0.0 LANGUAGES C CXX)

set(PFU_VERSION "${PROJECT_VERSION}" CACHE STRING "Version reported by every binary")

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_C_STANDARD 11)
set(CMAKE_AUTOMOC ON)

if(NOT CMAKE_BUILD_TYPE)
    set(CMAKE_BUILD_TYPE RelWithDebInfo)
endif()

include(GNUInstallDirs)

option(PFU_BUILD_AGENT "Build the Plasma agent (bubble, lock screen, set up window)" ON)
option(PFU_BUILD_PAM "Build the PAM module" ON)
option(PFU_BUILD_TESTS "Build the tests" ON)

# Where things go at run time. The defaults suit a normal install into /usr;
# the Makefile passes its own values so the two never disagree.
set(PFU_LIBEXECDIR "${CMAKE_INSTALL_PREFIX}/lib/plasma-face-unlock" CACHE PATH "Helper programs")
set(PFU_MODELDIR "${CMAKE_INSTALL_FULL_DATADIR}/plasma-face-unlock/models" CACHE PATH "Neural network models")
set(PFU_LOCALEDIR "${CMAKE_INSTALL_FULL_LOCALEDIR}" CACHE PATH "Translations")
set(PFU_SOCKET "/run/plasma-face-unlock/socket" CACHE STRING "The daemon's socket")
set(PFU_STATEDIR "/var/lib/plasma-face-unlock" CACHE PATH "Face data")
set(PFU_CONFIG "/etc/plasma-face-unlock/config" CACHE FILEPATH "System settings")
set(PFU_PAMDIR "${CMAKE_INSTALL_PREFIX}/lib/security" CACHE PATH "Where PAM modules live")

configure_file(src/shared/buildconfig.h.in ${CMAKE_CURRENT_BINARY_DIR}/buildconfig.h @ONLY)
include_directories(${CMAKE_CURRENT_BINARY_DIR} src/shared)

add_compile_options(-Wall -Wextra -Wno-unused-parameter)

# ---------------------------------------------------------------------------
# The core: camera, vision, liveness, store
# ---------------------------------------------------------------------------

find_package(Qt6 6.5 REQUIRED COMPONENTS Core DBus Network)
# OpenCV 5 split the contour and transform helpers into "geometry".
find_package(OpenCV REQUIRED COMPONENTS core)
set(PFU_OPENCV_MODULES core imgproc imgcodecs videoio objdetect dnn)
if(OpenCV_VERSION_MAJOR GREATER_EQUAL 5)
    list(APPEND PFU_OPENCV_MODULES geometry)
endif()
find_package(OpenCV REQUIRED COMPONENTS ${PFU_OPENCV_MODULES})

# The settings file reader, shared with the agent (which has no use for
# OpenCV).
add_library(pfu_common STATIC src/core/keyvalue.cpp)
target_include_directories(pfu_common PUBLIC src/core)
target_link_libraries(pfu_common PUBLIC Qt6::Core)
set_target_properties(pfu_common PROPERTIES POSITION_INDEPENDENT_CODE ON)

add_library(pfu_core STATIC
    src/core/settings.cpp
    src/core/camera.cpp
    src/core/vision.cpp
    src/core/liveness.cpp
    src/core/store.cpp
)
target_include_directories(pfu_core PUBLIC src/core ${OpenCV_INCLUDE_DIRS})
target_link_libraries(pfu_core PUBLIC pfu_common Qt6::Core ${OpenCV_LIBS})
set_target_properties(pfu_core PROPERTIES POSITION_INDEPENDENT_CODE ON)

# ---------------------------------------------------------------------------
# The daemon
# ---------------------------------------------------------------------------

add_executable(plasma-face-unlockd
    src/daemon/job.h
    src/daemon/agentlink.cpp
    src/daemon/main.cpp
    src/daemon/server.cpp
    src/daemon/client.cpp
    src/daemon/scanjob.cpp
    src/daemon/enrolljob.cpp
    src/daemon/polkit.cpp
    src/daemon/userstate.cpp
    src/daemon/system.cpp
)
target_link_libraries(plasma-face-unlockd PRIVATE pfu_core Qt6::DBus Qt6::Network)
install(TARGETS plasma-face-unlockd DESTINATION ${PFU_LIBEXECDIR})

# ---------------------------------------------------------------------------
# The client the shell code uses
# ---------------------------------------------------------------------------

add_executable(plasma-face-unlock-ctl src/ctl/main.cpp)
target_link_libraries(plasma-face-unlock-ctl PRIVATE Qt6::Core Qt6::Network)
install(TARGETS plasma-face-unlock-ctl DESTINATION ${PFU_LIBEXECDIR})

# ---------------------------------------------------------------------------
# The PAM module
# ---------------------------------------------------------------------------

if(PFU_BUILD_PAM)
    find_path(PAM_INCLUDE_DIR security/pam_modules.h REQUIRED)
    find_library(PAM_LIBRARY pam REQUIRED)
    find_package(PkgConfig REQUIRED)
    pkg_check_modules(SYSTEMD IMPORTED_TARGET libsystemd)

    add_library(pam_plasma_face_unlock MODULE src/pam/pam_plasma_face_unlock.c)
    set_target_properties(pam_plasma_face_unlock PROPERTIES PREFIX "" C_VISIBILITY_PRESET hidden)
    target_include_directories(pam_plasma_face_unlock PRIVATE ${PAM_INCLUDE_DIR})
    target_link_libraries(pam_plasma_face_unlock PRIVATE ${PAM_LIBRARY})
    if(SYSTEMD_FOUND)
        target_compile_definitions(pam_plasma_face_unlock PRIVATE HAVE_SYSTEMD=1)
        target_link_libraries(pam_plasma_face_unlock PRIVATE PkgConfig::SYSTEMD)
    endif()
    install(TARGETS pam_plasma_face_unlock DESTINATION ${PFU_PAMDIR})

    if(PFU_BUILD_TESTS)
        add_executable(pam_harness tests/pam_harness.c)
        target_include_directories(pam_harness PRIVATE ${PAM_INCLUDE_DIR})
        target_link_libraries(pam_harness PRIVATE ${PAM_LIBRARY})
    endif()
endif()

# ---------------------------------------------------------------------------
# The agent
# ---------------------------------------------------------------------------

if(PFU_BUILD_AGENT)
    find_package(Qt6 6.5 REQUIRED COMPONENTS Gui Quick WaylandClient)
    # The bubble needs the Wayland surface of its window, which only the
    # private API hands out. It is the same one Plasma itself uses for this.
    set(QT_NO_PRIVATE_MODULE_WARNING ON)
    find_package(Qt6 REQUIRED COMPONENTS GuiPrivate WaylandClientPrivate)
    find_package(LayerShellQt REQUIRED)
    find_package(KF6I18n REQUIRED)

    qt_add_executable(plasma-face-unlock-agent
        src/agent/main.cpp
        src/agent/daemonclient.cpp
        src/agent/userconfig.cpp
        src/agent/agentsocket.cpp
        src/agent/bubblewindow.cpp
        src/agent/bubblecontroller.cpp
        src/agent/lockcontroller.cpp
        src/agent/inputwatcher.cpp
        src/agent/enrollcontroller.cpp
    )
    target_include_directories(plasma-face-unlock-agent PRIVATE src/agent)
    if(LayerShellQt_VERSION VERSION_GREATER_EQUAL 6.6)
        target_compile_definitions(plasma-face-unlock-agent PRIVATE PFU_LAYERSHELL_HAS_SCREEN)
    endif()

    qt6_generate_wayland_protocol_client_sources(plasma-face-unlock-agent
        FILES ${CMAKE_CURRENT_SOURCE_DIR}/protocols/kde-lockscreen-overlay-v1.xml
              ${CMAKE_CURRENT_SOURCE_DIR}/protocols/ext-idle-notify-v1.xml)

    # The QML files sit next to the module's qmldir in the resources, so they
    # see each other (and the Theme singleton) without importing anything.
    # Under /qt/qml, which the engine searches, or an installed agent finds no
    # types (the build directory only works by its PlasmaFaceUnlock/qmldir).
    set(PFU_QML_FILES Theme FaceGlyph LockGlyph Checkmark Bubble TickRing Enroll PillButton)
    foreach(f ${PFU_QML_FILES})
        set_source_files_properties(src/agent/qml/${f}.qml PROPERTIES QT_RESOURCE_ALIAS ${f}.qml)
    endforeach()
    set_source_files_properties(src/agent/qml/Theme.qml PROPERTIES QT_QML_SINGLETON_TYPE TRUE)

    qt_add_qml_module(plasma-face-unlock-agent
        URI PlasmaFaceUnlock
        VERSION 1.0
        RESOURCE_PREFIX /qt/qml
        SOURCES
            src/agent/previewitem.cpp
            src/agent/previewitem.h
        QML_FILES
            src/agent/qml/Theme.qml
            src/agent/qml/FaceGlyph.qml
            src/agent/qml/LockGlyph.qml
            src/agent/qml/Checkmark.qml
            src/agent/qml/Bubble.qml
            src/agent/qml/TickRing.qml
            src/agent/qml/Enroll.qml
            src/agent/qml/PillButton.qml
    )

    target_link_libraries(plasma-face-unlock-agent PRIVATE
        Qt6::Gui Qt6::GuiPrivate Qt6::Quick Qt6::DBus Qt6::Network
        Qt6::WaylandClient Qt6::WaylandClientPrivate
        LayerShellQt::Interface
        KF6::I18n KF6::I18nQml
        pfu_common
    )
    install(TARGETS plasma-face-unlock-agent DESTINATION ${PFU_LIBEXECDIR})
endif()

# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

if(PFU_BUILD_TESTS)
    enable_testing()

    add_executable(test_liveness tests/test_liveness.cpp)
    target_link_libraries(test_liveness PRIVATE pfu_core)
    add_test(NAME liveness COMMAND test_liveness)

    add_executable(test_store tests/test_store.cpp)
    target_link_libraries(test_store PRIVATE pfu_core)
    add_test(NAME store COMMAND test_store)

    add_executable(test_images tests/test_images.cpp)
    target_link_libraries(test_images PRIVATE pfu_core)
endif()
