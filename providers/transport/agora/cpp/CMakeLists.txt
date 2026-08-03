cmake_minimum_required(VERSION 3.20)
project(VoxaAgora VERSION 0.1.0 LANGUAGES CXX)

include(GNUInstallDirs)

get_filename_component(VOXA_DEFAULT_SOURCE_ROOT "${CMAKE_CURRENT_SOURCE_DIR}/../../../.." ABSOLUTE)
set(VOXA_SOURCE_ROOT "${VOXA_DEFAULT_SOURCE_ROOT}" CACHE PATH
    "Voxa source checkout used for the public C/C++ ABI headers")
set(VOXA_AGORA_SDK_ROOT "" CACHE PATH "Agora Native C++ SDK root")
option(VOXA_ENABLE_AGORA "Compile the real Agora Native SDK backend" OFF)
set(VOXA_NODE_PACK_OUTPUT_ROOT
  "${CMAKE_CURRENT_BINARY_DIR}/native" CACHE PATH
  "Destination for generated Voxa Node Pack artifacts")

if(VOXA_ENABLE_AGORA)
  set(VOXA_AGORA_PROVIDER_MODE "agora-native")
else()
  set(VOXA_AGORA_PROVIDER_MODE "offline-stub")
endif()

add_library(voxa_agora STATIC src/agora_rtc_adapter.cc src/agora_shared_session.cc)
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
  if(APPLE)
    file(GLOB VOXA_AGORA_FRAMEWORKS
      "${VOXA_AGORA_SDK_ROOT}/AgoraRtcKit.xcframework/macos-*/AgoraRtcKit.framework")
    list(LENGTH VOXA_AGORA_FRAMEWORKS VOXA_AGORA_FRAMEWORK_COUNT)
    if(NOT VOXA_AGORA_FRAMEWORK_COUNT EQUAL 1)
      message(FATAL_ERROR
        "Agora macOS SDK root must contain one AgoraRtcKit.xcframework macOS slice. "
        "Run providers/transport/agora/cpp/download-macos-sdk.sh or see the Voxa Provider guide.")
    endif()
    list(GET VOXA_AGORA_FRAMEWORKS 0 VOXA_AGORA_LIBRARY)
    set(VOXA_AGORA_INCLUDE_DIR "${VOXA_AGORA_LIBRARY}/Headers")
    file(GLOB VOXA_AGORA_XCFRAMEWORK_SLICES
      "${VOXA_AGORA_SDK_ROOT}/*.xcframework/macos-*")
  else()
    find_path(VOXA_AGORA_INCLUDE_DIR IAgoraRtcEngine.h
      PATHS "${VOXA_AGORA_SDK_ROOT}" PATH_SUFFIXES include sdk/include REQUIRED)
    find_library(VOXA_AGORA_LIBRARY NAMES agora_rtc_sdk AgoraRtcKit
      PATHS "${VOXA_AGORA_SDK_ROOT}" PATH_SUFFIXES lib libs libs/x86_64 REQUIRED)
  endif()
  target_sources(voxa_agora PRIVATE src/agora_native_sdk.cc)
  target_compile_definitions(voxa_agora PRIVATE VOXA_ENABLE_AGORA=1)
  target_include_directories(voxa_agora PRIVATE "${VOXA_AGORA_INCLUDE_DIR}")
  target_link_libraries(voxa_agora PUBLIC "${VOXA_AGORA_LIBRARY}")
else()
  target_sources(voxa_agora PRIVATE src/agora_native_sdk_stub.cc)
endif()

add_library(voxa_agora_node_runtime SHARED src/agora_shared_session.cc)
target_compile_features(voxa_agora_node_runtime PUBLIC cxx_std_17)
target_include_directories(voxa_agora_node_runtime PUBLIC
  "$<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>"
  "$<BUILD_INTERFACE:${VOXA_SOURCE_ROOT}/cpp/include>")
if(APPLE)
  target_include_directories(voxa_agora_node_runtime SYSTEM PRIVATE
    "${VOXA_APPLE_SDK_PATH}/usr/include/c++/v1")
endif()
if(VOXA_ENABLE_AGORA)
  target_sources(voxa_agora_node_runtime PRIVATE src/agora_native_sdk.cc)
  target_compile_definitions(voxa_agora_node_runtime PRIVATE VOXA_ENABLE_AGORA=1)
  target_include_directories(voxa_agora_node_runtime PRIVATE "${VOXA_AGORA_INCLUDE_DIR}")
  target_link_libraries(voxa_agora_node_runtime PUBLIC "${VOXA_AGORA_LIBRARY}")
else()
  target_sources(voxa_agora_node_runtime PRIVATE src/agora_native_sdk_stub.cc)
endif()
set_target_properties(voxa_agora_node_runtime PROPERTIES
  OUTPUT_NAME voxa_agora_provider
  LIBRARY_OUTPUT_DIRECTORY "${VOXA_NODE_PACK_OUTPUT_ROOT}")
if(APPLE AND VOXA_ENABLE_AGORA)
  set_target_properties(voxa_agora_node_runtime PROPERTIES
    BUILD_RPATH "${VOXA_AGORA_XCFRAMEWORK_SLICES}")
endif()

function(add_agora_node_pack package)
  add_library(${package} MODULE "nodes/${package}/node.cpp")
  target_compile_features(${package} PRIVATE cxx_std_17)
  target_include_directories(${package} PRIVATE
    "${VOXA_SOURCE_ROOT}/cpp/include"
    "${CMAKE_CURRENT_SOURCE_DIR}/include")
  if(APPLE)
    target_include_directories(${package} SYSTEM PRIVATE
      "${VOXA_APPLE_SDK_PATH}/usr/include/c++/v1")
    set_target_properties(${package} PROPERTIES SUFFIX ".dylib")
  endif()
  target_link_libraries(${package} PRIVATE voxa_agora_node_runtime)
  if(APPLE)
    set_target_properties(${package} PROPERTIES
      BUILD_RPATH "@loader_path/..;${VOXA_AGORA_XCFRAMEWORK_SLICES}")
  elseif(UNIX)
    set_target_properties(${package} PROPERTIES BUILD_RPATH "$ORIGIN/..")
  endif()
  set_target_properties(${package} PROPERTIES
    OUTPUT_NAME voxa_node_pack
    LIBRARY_OUTPUT_DIRECTORY "${VOXA_NODE_PACK_OUTPUT_ROOT}/${package}")
  file(GENERATE
    OUTPUT "${VOXA_NODE_PACK_OUTPUT_ROOT}/${package}/provider-mode"
    CONTENT "${VOXA_AGORA_PROVIDER_MODE}\n")
endfunction()

add_agora_node_pack(agora_audio_source)
add_agora_node_pack(agora_audio_sink)
add_agora_node_pack(agora_data_source)
add_agora_node_pack(agora_data_sink)

install(DIRECTORY include/ DESTINATION "${CMAKE_INSTALL_INCLUDEDIR}")
install(TARGETS voxa_agora
  ARCHIVE DESTINATION "${CMAKE_INSTALL_LIBDIR}")
install(TARGETS voxa_agora_node_runtime LIBRARY DESTINATION "${CMAKE_INSTALL_LIBDIR}")
