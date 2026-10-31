package dev.jot.cli;

import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;

/** Isolated bootstrap CLI parser. It can be replaced with a library without changing commands. */
record CommandLine(String command, Path projectRoot, String module, List<String> runArguments,
                   boolean help, boolean version) {
    static CommandLine parse(String[] rawArguments) {
        List<String> arguments = new ArrayList<>(List.of(rawArguments));
        if (arguments.isEmpty() || arguments.getFirst().equals("--help")
                || arguments.getFirst().equals("-h")) {
            return new CommandLine("", null, null, List.of(), true, false);
        }
        if (arguments.getFirst().equals("--version") || arguments.getFirst().equals("-V")) {
            return new CommandLine("", null, null, List.of(), false, true);
        }

        String command = arguments.removeFirst();
        Path projectRoot = null;
        String module = null;
        List<String> runArguments = new ArrayList<>();
        boolean programArguments = false;
        for (int i = 0; i < arguments.size(); i++) {
            String argument = arguments.get(i);
            if (programArguments) {
                runArguments.add(argument);
            } else if (argument.equals("--")) {
                programArguments = true;
            } else if (argument.equals("--project")) {
                if (++i == arguments.size()) throw new IllegalArgumentException("--project needs a path");
                projectRoot = Path.of(arguments.get(i)).toAbsolutePath().normalize();
            } else if (argument.equals("--module")) {
                if (++i == arguments.size()) throw new IllegalArgumentException("--module needs a name");
                module = arguments.get(i);
            } else {
                throw new IllegalArgumentException("unknown argument: " + argument);
            }
        }
        return new CommandLine(command, projectRoot, module, List.copyOf(runArguments), false, false);
    }
}
