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

option(VOXA_ENABLE_AGORA "Build the Agora Native C++ RTC provider" OFF)
set(VOXA_AGORA_SDK_ROOT "" CACHE PATH "Agora Native C++ SDK root")
add_library(voxa_agora STATIC cpp/adapters/src/agora_rtc_adapter.cc)
add_library(Voxa::agora ALIAS voxa_agora)
target_compile_features(voxa_agora PUBLIC cxx_std_17)
target_include_directories(voxa_agora PUBLIC
  "$<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/cpp/include>"
  "$<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/cpp/adapters/include>"
  "$<INSTALL_INTERFACE:${CMAKE_INSTALL_INCLUDEDIR}>")
target_link_libraries(voxa_agora PUBLIC voxa_cpp)
if(APPLE)
  execute_process(COMMAND xcrun --show-sdk-path
    OUTPUT_VARIABLE VOXA_APPLE_SDK_PATH OUTPUT_STRIP_TRAILING_WHITESPACE)
  target_include_directories(voxa_agora SYSTEM PRIVATE
    "${VOXA_APPLE_SDK_PATH}/usr/include/c++/v1")
endif()
if(VOXA_ENABLE_AGORA)
  find_path(VOXA_AGORA_INCLUDE_DIR IAgoraRtcEngine.h
    PATHS "${VOXA_AGORA_SDK_ROOT}" PATH_SUFFIXES include sdk/include REQUIRED)
  find_library(VOXA_AGORA_LIBRARY NAMES agora_rtc_sdk AgoraRtcKit
    PATHS "${VOXA_AGORA_SDK_ROOT}" PATH_SUFFIXES lib libs libs/x86_64 REQUIRED)
  target_sources(voxa_agora PRIVATE cpp/adapters/src/agora_native_sdk.cc)
  target_compile_definitions(voxa_agora PRIVATE VOXA_ENABLE_AGORA=1)
  target_include_directories(voxa_agora PRIVATE "${VOXA_AGORA_INCLUDE_DIR}")
  target_link_libraries(voxa_agora PUBLIC "${VOXA_AGORA_LIBRARY}")
endif()

install(DIRECTORY cpp/include/ DESTINATION "${CMAKE_INSTALL_INCLUDEDIR}")
install(DIRECTORY cpp/adapters/include/ DESTINATION "${CMAKE_INSTALL_INCLUDEDIR}")
install(FILES "${VOXA_FFI_LIBRARY}" DESTINATION "${CMAKE_INSTALL_LIBDIR}")
install(TARGETS voxa_agora ARCHIVE DESTINATION "${CMAKE_INSTALL_LIBDIR}")

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
