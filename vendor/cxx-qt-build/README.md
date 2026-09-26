# Synveil CXX-Qt build patch

This directory carries the cxx-qt-build 0.10.0 crate source under its
upstream MIT OR Apache-2.0 license terms.

Synveil sorts the Qt module set before passing it to qt-build-utils. Upstream
stores that set in a randomized HashSet; iterating it changed the native Qt
link directive order and therefore changed release executable bytes between
otherwise identical builds.

No CXX-Qt APIs or generated C++ behavior are changed.
