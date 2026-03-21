package com.jot.samples.minimal;

public final class Main {
    public static void main(String[] args) {
        String name = args.length > 0 ? args[0] : "world";
        System.out.println("hello " + name);
    }
}
