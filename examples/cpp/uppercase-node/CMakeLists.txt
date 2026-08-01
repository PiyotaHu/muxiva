cmake_minimum_required(VERSION 3.20)
project(voxa_cpp_uppercase LANGUAGES CXX)

find_package(Voxa CONFIG REQUIRED)
add_executable(voxa_cpp_uppercase main.cpp)
target_link_libraries(voxa_cpp_uppercase PRIVATE Voxa::cpp)
