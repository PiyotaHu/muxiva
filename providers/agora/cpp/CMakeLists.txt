cmake_minimum_required(VERSION 3.20)
project(VoxaAgora VERSION 0.1.0 LANGUAGES CXX)

include(GNUInstallDirs)

get_filename_component(VOXA_DEFAULT_SOURCE_ROOT "${CMAKE_CURRENT_SOURCE_DIR}/../../.." ABSOLUTE)
set(VOXA_SOURCE_ROOT "${VOXA_DEFAULT_SOURCE_ROOT}" CACHE PATH
    "Voxa source checkout used for the public C/C++ ABI headers")
set(VOXA_AGORA_SDK_ROOT "" CACHE PATH "Agora Native C++ SDK root")
option(VOXA_ENABLE_AGORA "Compile the real Agora Native SDK backend" OFF)

add_library(voxa_agora STATIC src/agora_rtc_adapter.cc)
add_library(VoxaAgora::agora ALIAS voxa_agora)
target_compile_features(voxa_agora PUBLIC cxx_std_17)
target_include_directories(voxa_agora PUBLIC
  "$<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>"
  "$<BUILD_INTERFACE:${VOXA_SOURCE_ROOT}/cpp/include>"
  "$<INSTALL_INTERFACE:${CMAKE_INSTALL_INCLUDEDIR}>")

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
  target_sources(voxa_agora PRIVATE src/agora_native_sdk.cc)
  target_compile_definitions(voxa_agora PRIVATE VOXA_ENABLE_AGORA=1)
  target_include_directories(voxa_agora PRIVATE "${VOXA_AGORA_INCLUDE_DIR}")
  target_link_libraries(voxa_agora PUBLIC "${VOXA_AGORA_LIBRARY}")
else()
  target_sources(voxa_agora PRIVATE src/agora_native_sdk_stub.cc)
endif()

install(DIRECTORY include/ DESTINATION "${CMAKE_INSTALL_INCLUDEDIR}")
install(TARGETS voxa_agora ARCHIVE DESTINATION "${CMAKE_INSTALL_LIBDIR}")
