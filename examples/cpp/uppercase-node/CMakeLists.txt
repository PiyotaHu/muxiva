cmake_minimum_required(VERSION 3.20)
project(muxiva_cpp_uppercase LANGUAGES CXX)

find_package(Muxiva CONFIG REQUIRED)
add_executable(muxiva_cpp_uppercase main.cpp)
target_link_libraries(muxiva_cpp_uppercase PRIVATE Muxiva::cpp)
