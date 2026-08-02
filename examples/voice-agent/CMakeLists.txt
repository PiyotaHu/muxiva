cmake_minimum_required(VERSION 3.20)
project(VoxaVoiceAgentNativeNodes VERSION 0.1.0 LANGUAGES CXX)

get_filename_component(VOXA_SOURCE_ROOT "${CMAKE_CURRENT_SOURCE_DIR}/../.." ABSOLUTE)
set(VOXA_AGORA_SDK_ROOT "" CACHE PATH "Agora Native C++ SDK root")
option(VOXA_ENABLE_AGORA "Compile the real Agora Native SDK backend" OFF)
set(VOXA_NODE_PACK_OUTPUT_ROOT
  "${CMAKE_CURRENT_SOURCE_DIR}/.voxa/native" CACHE PATH
  "Destination for generated Voxa Node Pack artifacts")

if(VOXA_ENABLE_AGORA)
  set(VOXA_AGORA_PROVIDER_MODE "agora-native")
else()
  set(VOXA_AGORA_PROVIDER_MODE "offline-stub")
endif()

if(APPLE)
  execute_process(COMMAND xcrun --show-sdk-path
    OUTPUT_VARIABLE VOXA_APPLE_SDK_PATH OUTPUT_STRIP_TRAILING_WHITESPACE)
endif()

if(VOXA_ENABLE_AGORA)
  find_path(VOXA_AGORA_INCLUDE_DIR IAgoraRtcEngine.h
    PATHS "${VOXA_AGORA_SDK_ROOT}" PATH_SUFFIXES include sdk/include REQUIRED)
  find_library(VOXA_AGORA_LIBRARY NAMES agora_rtc_sdk AgoraRtcKit
    PATHS "${VOXA_AGORA_SDK_ROOT}" PATH_SUFFIXES lib libs libs/x86_64 REQUIRED)
endif()

function(add_agora_node_pack package source)
  add_library(${package} MODULE "${source}")
  target_compile_features(${package} PRIVATE cxx_std_17)
  target_include_directories(${package} PRIVATE
    "${VOXA_SOURCE_ROOT}/cpp/include"
    "${VOXA_SOURCE_ROOT}/providers/agora/cpp/include")
  if(APPLE)
    target_include_directories(${package} SYSTEM PRIVATE
      "${VOXA_APPLE_SDK_PATH}/usr/include/c++/v1")
    set_target_properties(${package} PROPERTIES SUFFIX ".dylib")
  endif()
  if(VOXA_ENABLE_AGORA)
    target_sources(${package} PRIVATE
      "${VOXA_SOURCE_ROOT}/providers/agora/cpp/src/agora_native_sdk.cc")
    target_compile_definitions(${package} PRIVATE VOXA_ENABLE_AGORA=1)
    target_include_directories(${package} PRIVATE "${VOXA_AGORA_INCLUDE_DIR}")
    target_link_libraries(${package} PRIVATE "${VOXA_AGORA_LIBRARY}")
  else()
    target_sources(${package} PRIVATE
      "${VOXA_SOURCE_ROOT}/providers/agora/cpp/src/agora_native_sdk_stub.cc")
  endif()
  set_target_properties(${package} PROPERTIES
    OUTPUT_NAME voxa_node_pack
    LIBRARY_OUTPUT_DIRECTORY
      "${VOXA_NODE_PACK_OUTPUT_ROOT}/${package}")
  file(GENERATE
    OUTPUT "${VOXA_NODE_PACK_OUTPUT_ROOT}/${package}/provider-mode"
    CONTENT "${VOXA_AGORA_PROVIDER_MODE}\n")
endfunction()

add_agora_node_pack(agora_audio_source
  ".voxa/nodes/agora_audio_source/node.cpp")
add_agora_node_pack(agora_audio_sink
  ".voxa/nodes/agora_audio_sink/node.cpp")
