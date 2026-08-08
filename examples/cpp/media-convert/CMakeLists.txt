cmake_minimum_required(VERSION 3.20)
project(muxiva_media_convert LANGUAGES CXX)

find_package(Muxiva CONFIG REQUIRED)
add_executable(muxiva_media_convert main.cpp)
target_link_libraries(muxiva_media_convert PRIVATE Muxiva::media)

