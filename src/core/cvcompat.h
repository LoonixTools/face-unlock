// SPDX-License-Identifier: GPL-3.0-or-later
//
// OpenCV 5 moved the contour and transform helpers (getPerspectiveTransform,
// approxPolyDP and friends) out of imgproc into a module of their own. The
// distributions this builds on ship 4.x and 5.x, so both are included here.

#pragma once

#include <opencv2/core/version.hpp>
#include <opencv2/imgproc.hpp>

#if CV_VERSION_MAJOR >= 5
#include <opencv2/geometry.hpp>
#endif
