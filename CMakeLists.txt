cmake_minimum_required(VERSION 3.20)
project(Muxiva VERSION 0.1.0 LANGUAGES C CXX)

include(GNUInstallDirs)
include(CMakePackageConfigHelpers)

find_program(MUXIVA_CARGO_EXECUTABLE cargo REQUIRED)
set(MUXIVA_REPOSITORY_ROOT "${CMAKE_CURRENT_SOURCE_DIR}")
set(MUXIVA_CARGO_TARGET_DIR "${CMAKE_CURRENT_BINARY_DIR}/cargo-target")

if(APPLE)
  set(MUXIVA_FFI_FILENAME "libmuxiva_ffi.dylib")
elseif(UNIX)
  set(MUXIVA_FFI_FILENAME "libmuxiva_ffi.so")
else()
  message(FATAL_ERROR "The CMake SDK installer currently supports macOS and Linux")
endif()

set(MUXIVA_FFI_LIBRARY "${MUXIVA_CARGO_TARGET_DIR}/release/${MUXIVA_FFI_FILENAME}")
add_custom_target(muxiva_ffi ALL
  COMMAND "${CMAKE_COMMAND}" -E env "CARGO_TARGET_DIR=${MUXIVA_CARGO_TARGET_DIR}"
          "${MUXIVA_CARGO_EXECUTABLE}" build --locked --release -p muxiva-ffi
          --manifest-path "${MUXIVA_REPOSITORY_ROOT}/Cargo.toml"
  BYPRODUCTS "${MUXIVA_FFI_LIBRARY}"
  WORKING_DIRECTORY "${MUXIVA_REPOSITORY_ROOT}"
  COMMENT "Building the Muxiva C ABI runtime with Cargo"
  VERBATIM)

add_library(muxiva_cpp INTERFACE)
add_library(Muxiva::cpp ALIAS muxiva_cpp)
add_dependencies(muxiva_cpp muxiva_ffi)
target_compile_features(muxiva_cpp INTERFACE cxx_std_17)
target_include_directories(muxiva_cpp INTERFACE
  "$<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/cpp/include>"
  "$<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/cpp/adapters/include>"
  "$<INSTALL_INTERFACE:${CMAKE_INSTALL_INCLUDEDIR}>")
target_link_libraries(muxiva_cpp INTERFACE "${MUXIVA_FFI_LIBRARY}")

option(MUXIVA_ENABLE_FFMPEG "Build the FFmpeg media conversion backend" OFF)
set(MUXIVA_FFMPEG_ROOT "" CACHE PATH "FFmpeg installation prefix")
add_library(muxiva_media STATIC cpp/media/src/media_pipeline.cc)
add_library(Muxiva::media ALIAS muxiva_media)
target_compile_features(muxiva_media PUBLIC cxx_std_17)
target_include_directories(muxiva_media PUBLIC
  "$<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/cpp/media/include>"
  "$<INSTALL_INTERFACE:${CMAKE_INSTALL_INCLUDEDIR}>")
if(APPLE)
  execute_process(COMMAND xcrun --show-sdk-path
    OUTPUT_VARIABLE MUXIVA_APPLE_SDK_PATH OUTPUT_STRIP_TRAILING_WHITESPACE)
  target_include_directories(muxiva_media SYSTEM PRIVATE
    "${MUXIVA_APPLE_SDK_PATH}/usr/include/c++/v1")
endif()
if(MUXIVA_ENABLE_FFMPEG)
  find_path(MUXIVA_FFMPEG_INCLUDE_DIR libavutil/avutil.h
    PATHS "${MUXIVA_FFMPEG_ROOT}" PATH_SUFFIXES include REQUIRED)
  find_library(MUXIVA_AVUTIL_LIBRARY NAMES avutil
    PATHS "${MUXIVA_FFMPEG_ROOT}" PATH_SUFFIXES lib REQUIRED)
  find_library(MUXIVA_SWRESAMPLE_LIBRARY NAMES swresample
    PATHS "${MUXIVA_FFMPEG_ROOT}" PATH_SUFFIXES lib REQUIRED)
  find_library(MUXIVA_SWSCALE_LIBRARY NAMES swscale
    PATHS "${MUXIVA_FFMPEG_ROOT}" PATH_SUFFIXES lib REQUIRED)
  target_sources(muxiva_media PRIVATE cpp/media/src/ffmpeg_backend.cc)
  target_compile_definitions(muxiva_media PRIVATE MUXIVA_ENABLE_FFMPEG=1)
  target_include_directories(muxiva_media PRIVATE "${MUXIVA_FFMPEG_INCLUDE_DIR}")
  target_link_libraries(muxiva_media PUBLIC
    "${MUXIVA_AVUTIL_LIBRARY}" "${MUXIVA_SWRESAMPLE_LIBRARY}" "${MUXIVA_SWSCALE_LIBRARY}")
endif()

install(DIRECTORY cpp/include/ DESTINATION "${CMAKE_INSTALL_INCLUDEDIR}")
install(DIRECTORY cpp/adapters/include/ DESTINATION "${CMAKE_INSTALL_INCLUDEDIR}")
install(DIRECTORY cpp/media/include/ DESTINATION "${CMAKE_INSTALL_INCLUDEDIR}")
install(FILES "${MUXIVA_FFI_LIBRARY}" DESTINATION "${CMAKE_INSTALL_LIBDIR}")
install(TARGETS muxiva_media ARCHIVE DESTINATION "${CMAKE_INSTALL_LIBDIR}")

configure_package_config_file(
  cmake/MuxivaConfig.cmake.in
  "${CMAKE_CURRENT_BINARY_DIR}/MuxivaConfig.cmake"
  INSTALL_DESTINATION "${CMAKE_INSTALL_LIBDIR}/cmake/Muxiva")
write_basic_package_version_file(
  "${CMAKE_CURRENT_BINARY_DIR}/MuxivaConfigVersion.cmake"
  VERSION "${PROJECT_VERSION}"
  COMPATIBILITY SameMajorVersion)
install(FILES
  "${CMAKE_CURRENT_BINARY_DIR}/MuxivaConfig.cmake"
  "${CMAKE_CURRENT_BINARY_DIR}/MuxivaConfigVersion.cmake"
  DESTINATION "${CMAKE_INSTALL_LIBDIR}/cmake/Muxiva")
