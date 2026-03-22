package com.jot.samples.mixed;

public class Main {
    public static void main(String[] args) {
        String name = args.length > 0 ? args[0] : "World";
        Greeter greeter = new Greeter(name);
        System.out.println(greeter.greet());
    }
}
