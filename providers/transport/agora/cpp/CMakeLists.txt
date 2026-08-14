cmake_minimum_required(VERSION 3.20)
project(MuxivaAgora VERSION 0.1.0 LANGUAGES CXX)

include(GNUInstallDirs)

get_filename_component(MUXIVA_DEFAULT_SOURCE_ROOT "${CMAKE_CURRENT_SOURCE_DIR}/../../../.." ABSOLUTE)
set(MUXIVA_SOURCE_ROOT "${MUXIVA_DEFAULT_SOURCE_ROOT}" CACHE PATH
    "Muxiva source checkout used for the public C/C++ ABI headers")
set(MUXIVA_AGORA_SDK_ROOT "" CACHE PATH "Agora Native C++ SDK root")
option(MUXIVA_ENABLE_AGORA "Compile the real Agora Native SDK backend" OFF)
set(MUXIVA_NODE_PACK_OUTPUT_ROOT
  "${CMAKE_CURRENT_BINARY_DIR}/native" CACHE PATH
  "Destination for generated Muxiva Node Pack artifacts")

if(MUXIVA_ENABLE_AGORA)
  set(MUXIVA_AGORA_PROVIDER_MODE "agora-native")
else()
  set(MUXIVA_AGORA_PROVIDER_MODE "offline-stub")
endif()

# The vendor API differs by platform: macOS uses the Agora client SDK
# (IAgoraRtcEngine); Linux uses the Server Gateway / RTSA SDK (IRtcConnection).
if(APPLE)
  set(MUXIVA_AGORA_NATIVE_SDK_SOURCE src/agora_native_sdk.cc)
else()
  set(MUXIVA_AGORA_NATIVE_SDK_SOURCE src/agora_native_sdk_linux.cc)
endif()

add_library(muxiva_agora STATIC src/agora_rtc_adapter.cc src/agora_shared_session.cc)
add_library(MuxivaAgora::agora ALIAS muxiva_agora)
target_compile_features(muxiva_agora PUBLIC cxx_std_17)
target_include_directories(muxiva_agora PUBLIC
  "$<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>"
  "$<BUILD_INTERFACE:${MUXIVA_SOURCE_ROOT}/cpp/include>"
  "$<INSTALL_INTERFACE:${CMAKE_INSTALL_INCLUDEDIR}>")

if(APPLE)
  execute_process(COMMAND xcrun --show-sdk-path
    OUTPUT_VARIABLE MUXIVA_APPLE_SDK_PATH OUTPUT_STRIP_TRAILING_WHITESPACE)
  target_include_directories(muxiva_agora SYSTEM PRIVATE
    "${MUXIVA_APPLE_SDK_PATH}/usr/include/c++/v1")
endif()

if(MUXIVA_ENABLE_AGORA)
  if(APPLE)
    file(GLOB MUXIVA_AGORA_FRAMEWORKS
      "${MUXIVA_AGORA_SDK_ROOT}/AgoraRtcKit.xcframework/macos-*/AgoraRtcKit.framework")
    list(LENGTH MUXIVA_AGORA_FRAMEWORKS MUXIVA_AGORA_FRAMEWORK_COUNT)
    if(NOT MUXIVA_AGORA_FRAMEWORK_COUNT EQUAL 1)
      message(FATAL_ERROR
        "Agora macOS SDK root must contain one AgoraRtcKit.xcframework macOS slice. "
        "Run providers/transport/agora/cpp/download-macos-sdk.sh or see the Muxiva Provider guide.")
    endif()
    list(GET MUXIVA_AGORA_FRAMEWORKS 0 MUXIVA_AGORA_LIBRARY)
    set(MUXIVA_AGORA_INCLUDE_DIR "${MUXIVA_AGORA_LIBRARY}/Headers")
    file(GLOB MUXIVA_AGORA_XCFRAMEWORK_SLICES
      "${MUXIVA_AGORA_SDK_ROOT}/*.xcframework/macos-*")
  else()
    find_path(MUXIVA_AGORA_INCLUDE_DIR IAgoraService.h
      PATHS "${MUXIVA_AGORA_SDK_ROOT}" PATH_SUFFIXES agora_sdk/include include sdk/include REQUIRED)
    find_library(MUXIVA_AGORA_LIBRARY NAMES agora_rtc_sdk AgoraRtcKit
      PATHS "${MUXIVA_AGORA_SDK_ROOT}" PATH_SUFFIXES agora_sdk lib libs libs/x86_64 REQUIRED)
    get_filename_component(MUXIVA_AGORA_LIBRARY_DIR "${MUXIVA_AGORA_LIBRARY}" DIRECTORY)
  endif()
  target_sources(muxiva_agora PRIVATE "${MUXIVA_AGORA_NATIVE_SDK_SOURCE}")
  target_compile_definitions(muxiva_agora PRIVATE MUXIVA_ENABLE_AGORA=1)
  target_include_directories(muxiva_agora PRIVATE "${MUXIVA_AGORA_INCLUDE_DIR}")
  target_link_libraries(muxiva_agora PUBLIC "${MUXIVA_AGORA_LIBRARY}")
else()
  target_sources(muxiva_agora PRIVATE src/agora_native_sdk_stub.cc)
endif()

add_library(muxiva_agora_node_runtime SHARED src/agora_shared_session.cc)
target_compile_features(muxiva_agora_node_runtime PUBLIC cxx_std_17)
target_include_directories(muxiva_agora_node_runtime PUBLIC
  "$<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>"
  "$<BUILD_INTERFACE:${MUXIVA_SOURCE_ROOT}/cpp/include>")
if(APPLE)
  target_include_directories(muxiva_agora_node_runtime SYSTEM PRIVATE
    "${MUXIVA_APPLE_SDK_PATH}/usr/include/c++/v1")
endif()
if(MUXIVA_ENABLE_AGORA)
  target_sources(muxiva_agora_node_runtime PRIVATE "${MUXIVA_AGORA_NATIVE_SDK_SOURCE}")
  target_compile_definitions(muxiva_agora_node_runtime PRIVATE MUXIVA_ENABLE_AGORA=1)
  target_include_directories(muxiva_agora_node_runtime PRIVATE "${MUXIVA_AGORA_INCLUDE_DIR}")
  target_link_libraries(muxiva_agora_node_runtime PUBLIC "${MUXIVA_AGORA_LIBRARY}")
else()
  target_sources(muxiva_agora_node_runtime PRIVATE src/agora_native_sdk_stub.cc)
endif()
set_target_properties(muxiva_agora_node_runtime PROPERTIES
  OUTPUT_NAME muxiva_agora_provider
  LIBRARY_OUTPUT_DIRECTORY "${MUXIVA_NODE_PACK_OUTPUT_ROOT}")
if(APPLE AND MUXIVA_ENABLE_AGORA)
  set_target_properties(muxiva_agora_node_runtime PROPERTIES
    BUILD_RPATH "${MUXIVA_AGORA_XCFRAMEWORK_SLICES}")
elseif(UNIX AND MUXIVA_ENABLE_AGORA)
  set_target_properties(muxiva_agora_node_runtime PROPERTIES
    BUILD_RPATH "${MUXIVA_AGORA_LIBRARY_DIR}")
  # Use DT_RPATH (transitive) so libagora_rtc_sdk.so's sibling libraries
  # (libagora-fdkaac.so, libaosl.so) resolve from the same vendor directory.
  target_link_options(muxiva_agora_node_runtime PRIVATE
    "-Wl,--disable-new-dtags")
endif()

function(add_agora_node_pack package)
  add_library(${package} MODULE "nodes/${package}/node.cpp")
  target_compile_features(${package} PRIVATE cxx_std_17)
  target_include_directories(${package} PRIVATE
    "${MUXIVA_SOURCE_ROOT}/cpp/include"
    "${CMAKE_CURRENT_SOURCE_DIR}/include")
  if(APPLE)
    target_include_directories(${package} SYSTEM PRIVATE
      "${MUXIVA_APPLE_SDK_PATH}/usr/include/c++/v1")
    set_target_properties(${package} PROPERTIES SUFFIX ".dylib")
  endif()
  target_link_libraries(${package} PRIVATE muxiva_agora_node_runtime)
  if(APPLE)
    set_target_properties(${package} PROPERTIES
      BUILD_RPATH "@loader_path/..;${MUXIVA_AGORA_XCFRAMEWORK_SLICES}")
  elseif(UNIX AND MUXIVA_ENABLE_AGORA)
    set_target_properties(${package} PROPERTIES
      BUILD_RPATH "$ORIGIN/..;${MUXIVA_AGORA_LIBRARY_DIR}")
    target_link_options(${package} PRIVATE "-Wl,--disable-new-dtags")
  elseif(UNIX)
    set_target_properties(${package} PROPERTIES BUILD_RPATH "$ORIGIN/..")
  endif()
  set_target_properties(${package} PROPERTIES
    OUTPUT_NAME muxiva_node_pack
    LIBRARY_OUTPUT_DIRECTORY "${MUXIVA_NODE_PACK_OUTPUT_ROOT}/${package}")
  file(GENERATE
    OUTPUT "${MUXIVA_NODE_PACK_OUTPUT_ROOT}/${package}/provider-mode"
    CONTENT "${MUXIVA_AGORA_PROVIDER_MODE}\n")
endfunction()

add_agora_node_pack(agora_audio_source)
add_agora_node_pack(agora_audio_sink)
add_agora_node_pack(agora_data_source)
add_agora_node_pack(agora_data_sink)

install(DIRECTORY include/ DESTINATION "${CMAKE_INSTALL_INCLUDEDIR}")
install(TARGETS muxiva_agora
  ARCHIVE DESTINATION "${CMAKE_INSTALL_LIBDIR}")
install(TARGETS muxiva_agora_node_runtime LIBRARY DESTINATION "${CMAKE_INSTALL_LIBDIR}")
