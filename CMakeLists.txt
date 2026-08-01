cmake_minimum_required(VERSION 3.20)
project(Voxa VERSION 0.1.0 LANGUAGES C CXX)

include(GNUInstallDirs)
include(CMakePackageConfigHelpers)

find_program(VOXA_CARGO_EXECUTABLE cargo REQUIRED)
set(VOXA_REPOSITORY_ROOT "${CMAKE_CURRENT_SOURCE_DIR}")
set(VOXA_CARGO_TARGET_DIR "${CMAKE_CURRENT_BINARY_DIR}/cargo-target")

if(APPLE)
  set(VOXA_FFI_FILENAME "libvoxa_ffi.dylib")
elseif(UNIX)
  set(VOXA_FFI_FILENAME "libvoxa_ffi.so")
else()
  message(FATAL_ERROR "The CMake SDK installer currently supports macOS and Linux")
endif()

set(VOXA_FFI_LIBRARY "${VOXA_CARGO_TARGET_DIR}/release/${VOXA_FFI_FILENAME}")
add_custom_target(voxa_ffi ALL
  COMMAND "${CMAKE_COMMAND}" -E env "CARGO_TARGET_DIR=${VOXA_CARGO_TARGET_DIR}"
          "${VOXA_CARGO_EXECUTABLE}" build --locked --release -p voxa-ffi
          --manifest-path "${VOXA_REPOSITORY_ROOT}/Cargo.toml"
  BYPRODUCTS "${VOXA_FFI_LIBRARY}"
  WORKING_DIRECTORY "${VOXA_REPOSITORY_ROOT}"
  COMMENT "Building the Voxa C ABI runtime with Cargo"
  VERBATIM)

add_library(voxa_cpp INTERFACE)
add_library(Voxa::cpp ALIAS voxa_cpp)
add_dependencies(voxa_cpp voxa_ffi)
target_compile_features(voxa_cpp INTERFACE cxx_std_17)
target_include_directories(voxa_cpp INTERFACE
  "$<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/cpp/include>"
  "$<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/cpp/adapters/include>"
  "$<INSTALL_INTERFACE:${CMAKE_INSTALL_INCLUDEDIR}>")
target_link_libraries(voxa_cpp INTERFACE "${VOXA_FFI_LIBRARY}")

option(VOXA_ENABLE_FFMPEG "Build the FFmpeg media conversion backend" OFF)
set(VOXA_FFMPEG_ROOT "" CACHE PATH "FFmpeg installation prefix")
add_library(voxa_media STATIC cpp/media/src/media_pipeline.cc)
add_library(Voxa::media ALIAS voxa_media)
target_compile_features(voxa_media PUBLIC cxx_std_17)
target_include_directories(voxa_media PUBLIC
  "$<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/cpp/media/include>"
  "$<INSTALL_INTERFACE:${CMAKE_INSTALL_INCLUDEDIR}>")
if(APPLE)
  execute_process(COMMAND xcrun --show-sdk-path
    OUTPUT_VARIABLE VOXA_APPLE_SDK_PATH OUTPUT_STRIP_TRAILING_WHITESPACE)
  target_include_directories(voxa_media SYSTEM PRIVATE
    "${VOXA_APPLE_SDK_PATH}/usr/include/c++/v1")
endif()
if(VOXA_ENABLE_FFMPEG)
  find_path(VOXA_FFMPEG_INCLUDE_DIR libavutil/avutil.h
    PATHS "${VOXA_FFMPEG_ROOT}" PATH_SUFFIXES include REQUIRED)
  find_library(VOXA_AVUTIL_LIBRARY NAMES avutil
    PATHS "${VOXA_FFMPEG_ROOT}" PATH_SUFFIXES lib REQUIRED)
  find_library(VOXA_SWRESAMPLE_LIBRARY NAMES swresample
    PATHS "${VOXA_FFMPEG_ROOT}" PATH_SUFFIXES lib REQUIRED)
  find_library(VOXA_SWSCALE_LIBRARY NAMES swscale
    PATHS "${VOXA_FFMPEG_ROOT}" PATH_SUFFIXES lib REQUIRED)
  target_sources(voxa_media PRIVATE cpp/media/src/ffmpeg_backend.cc)
  target_compile_definitions(voxa_media PRIVATE VOXA_ENABLE_FFMPEG=1)
  target_include_directories(voxa_media PRIVATE "${VOXA_FFMPEG_INCLUDE_DIR}")
  target_link_libraries(voxa_media PUBLIC
    "${VOXA_AVUTIL_LIBRARY}" "${VOXA_SWRESAMPLE_LIBRARY}" "${VOXA_SWSCALE_LIBRARY}")
endif()

install(DIRECTORY cpp/include/ DESTINATION "${CMAKE_INSTALL_INCLUDEDIR}")
install(DIRECTORY cpp/adapters/include/ DESTINATION "${CMAKE_INSTALL_INCLUDEDIR}")
install(DIRECTORY cpp/media/include/ DESTINATION "${CMAKE_INSTALL_INCLUDEDIR}")
install(FILES "${VOXA_FFI_LIBRARY}" DESTINATION "${CMAKE_INSTALL_LIBDIR}")
install(TARGETS voxa_media ARCHIVE DESTINATION "${CMAKE_INSTALL_LIBDIR}")

configure_package_config_file(
  cmake/VoxaConfig.cmake.in
  "${CMAKE_CURRENT_BINARY_DIR}/VoxaConfig.cmake"
  INSTALL_DESTINATION "${CMAKE_INSTALL_LIBDIR}/cmake/Voxa")
write_basic_package_version_file(
  "${CMAKE_CURRENT_BINARY_DIR}/VoxaConfigVersion.cmake"
  VERSION "${PROJECT_VERSION}"
  COMPATIBILITY SameMajorVersion)
install(FILES
  "${CMAKE_CURRENT_BINARY_DIR}/VoxaConfig.cmake"
  "${CMAKE_CURRENT_BINARY_DIR}/VoxaConfigVersion.cmake"
  DESTINATION "${CMAKE_INSTALL_LIBDIR}/cmake/Voxa")
