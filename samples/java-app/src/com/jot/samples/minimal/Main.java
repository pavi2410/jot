package com.jot.samples.minimal;

import picocli.CommandLine;
import picocli.CommandLine.Command;
import picocli.CommandLine.Parameters;

@Command(name = "greet", mixinStandardHelpOptions = true, description = "Prints a greeting.")
public final class Main implements Runnable {

    @Parameters(index = "0", description = "Name to greet.", defaultValue = "world")
    private String name;

    @Override
    public void run() {
        System.out.println("hello " + name);
    }

    public static void main(String[] args) {
        System.exit(new CommandLine(new Main()).execute(args));
    }
}
