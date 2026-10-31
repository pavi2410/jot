package dev.jot.cli;

import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;

/** Parsed command and the options shared by the bootstrap CLI. */
record CommandLine(String command, Path projectRoot, String module, List<String> runArguments,
                   boolean help, boolean version, boolean offline, String projectName,
                   String template, String packageName, String dependencyCoordinate,
                   String dependencyName, String dependencyCatalog, boolean testDependency) {
    static CommandLine parse(String[] arguments) {
        Arguments input = new Arguments(arguments);
        if (!input.hasNext()) return helpCommand();

        boolean offline = false;
        while (input.peekIs("--offline")) {
            input.next();
            offline = true;
        }
        if (!input.hasNext() || isHelp(input.peek())) return helpCommand();
        if (isVersion(input.peek())) return versionCommand();

        String command = input.next();
        Options options = new Options(command, offline);
        while (input.hasNext()) {
            String token = input.next();
            if (token.equals("--") && command.equals("run")) {
                options.runArguments.addAll(input.remaining());
                break;
            }
            if (token.startsWith("--")) {
                options.parse(token, input);
            } else {
                options.parsePositional(token);
            }
        }
        options.validate();
        return options.toCommandLine();
    }

    private static boolean isHelp(String argument) {
        return argument.equals("--help") || argument.equals("-h");
    }

    private static boolean isVersion(String argument) {
        return argument.equals("--version") || argument.equals("-V");
    }

    private static CommandLine helpCommand() {
        return new CommandLine("", null, null, List.of(), true, false, false,
                null, null, null, null, null, null, false);
    }

    private static CommandLine versionCommand() {
        return new CommandLine("", null, null, List.of(), false, true, false,
                null, null, null, null, null, null, false);
    }

    private static final class Options {
        private final String command;
        private final List<String> runArguments = new ArrayList<>();
        private Path projectRoot;
        private String module;
        private String projectName;
        private String template;
        private String packageName;
        private String dependencyCoordinate;
        private String dependencyName;
        private String dependencyCatalog;
        private boolean offline;
        private boolean testDependency;

        private Options(String command, boolean offline) {
            this.command = command;
            this.offline = offline;
        }

        private void parse(String option, Arguments input) {
            switch (option) {
                case "--project" -> projectRoot = Path.of(input.value(option)).toAbsolutePath().normalize();
                case "--module" -> module = input.value(option);
                case "--offline" -> offline = true;
                case "--test" -> {
                    require(command.equals("add") || command.equals("remove"), option);
                    testDependency = true;
                }
                case "--name" -> {
                    require(command.equals("add"), option);
                    dependencyName = input.value(option);
                }
                case "--catalog" -> {
                    require(command.equals("add"), option);
                    dependencyCatalog = input.value(option);
                }
                case "--template" -> {
                    require(command.equals("init"), option);
                    template = input.value(option);
                }
                case "--package" -> {
                    require(command.equals("init"), option);
                    packageName = input.value(option);
                }
                default -> throw new IllegalArgumentException("unknown argument: " + option);
            }
        }

        private void parsePositional(String value) {
            switch (command) {
                case "init" -> {
                    if (projectName != null) throw new IllegalArgumentException("init accepts one project name");
                    projectName = value;
                }
                case "add" -> {
                    if (dependencyCoordinate != null) {
                        throw new IllegalArgumentException("add accepts one dependency coordinate");
                    }
                    dependencyCoordinate = value;
                }
                case "remove" -> {
                    if (dependencyName != null) throw new IllegalArgumentException("remove accepts one dependency name");
                    dependencyName = value;
                }
                default -> throw new IllegalArgumentException("unknown argument: " + value);
            }
        }

        private void validate() {
            if (command.equals("add") && dependencyCoordinate == null && dependencyCatalog == null) {
                throw new IllegalArgumentException("add needs a coordinate or --catalog alias");
            }
            if (command.equals("add") && dependencyCoordinate != null && dependencyCatalog != null) {
                throw new IllegalArgumentException("use either a dependency coordinate or --catalog");
            }
            if (command.equals("remove") && dependencyName == null) {
                throw new IllegalArgumentException("remove needs a dependency name");
            }
        }

        private CommandLine toCommandLine() {
            return new CommandLine(command, projectRoot, module, List.copyOf(runArguments),
                    false, false, offline, projectName, template, packageName,
                    dependencyCoordinate, dependencyName, dependencyCatalog, testDependency);
        }

        private static void require(boolean condition, String option) {
            if (!condition) throw new IllegalArgumentException("unknown argument: " + option);
        }
    }

    private static final class Arguments {
        private final String[] values;
        private int index;

        private Arguments(String[] values) {
            this.values = values;
        }

        private boolean hasNext() {
            return index < values.length;
        }

        private String peek() {
            return values[index];
        }

        private boolean peekIs(String value) {
            return hasNext() && peek().equals(value);
        }

        private String next() {
            return values[index++];
        }

        private String value(String option) {
            if (!hasNext() || peek().equals("--")) {
                throw new IllegalArgumentException(option + " needs a value");
            }
            return next();
        }

        private List<String> remaining() {
            List<String> remaining = List.of(values).subList(index, values.length);
            index = values.length;
            return remaining;
        }
    }
}
