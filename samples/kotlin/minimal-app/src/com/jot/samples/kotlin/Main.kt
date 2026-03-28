package com.jot.samples.kotlin

fun main(args: Array<String>) {
    val name = args.firstOrNull() ?: "World"
    println("Hello, $name!")
}
