cmake_minimum_required(VERSION 3.20)
project(voxa_media_convert LANGUAGES CXX)

find_package(Voxa CONFIG REQUIRED)
add_executable(voxa_media_convert main.cpp)
target_link_libraries(voxa_media_convert PRIVATE Voxa::media)

