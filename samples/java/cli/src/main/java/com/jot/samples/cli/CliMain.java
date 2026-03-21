package com.jot.samples.cli;

import picocli.CommandLine;
import picocli.CommandLine.Command;
import picocli.CommandLine.Parameters;

@Command(name = "cli", mixinStandardHelpOptions = true, description = "Greets the requested user.")
public final class CliMain implements Runnable {
    @Parameters(index = "0", defaultValue = "jot", description = "Name to greet.")
    private String name;

    public static void main(String[] args) {
        int exitCode = new CommandLine(new CliMain()).execute(args);
        System.exit(exitCode);
    }

    @Override
    public void run() {
        System.out.println("hello from cli, " + name);
    }
}
