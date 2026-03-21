package com.jot.samples.cli;

public final class CliMain {
    public static void main(String[] args) {
        if (args.length > 0 && "--help".equals(args[0])) {
            System.out.println("usage: cli [name]");
            return;
        }

        String name = args.length > 0 ? args[0] : "jot";
        System.out.println("hello from cli, " + name);
    }
}
