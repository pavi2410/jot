package dev.jot.cli;

import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;

/** Small, isolated command-line parser for the bootstrap implementation. */
record CommandLine(String command, Path projectRoot, String module, List<String> runArguments,
                   boolean help, boolean version, boolean offline, String projectName,
                   String template, String packageName) {
    static CommandLine parse(String[] arguments) {
        if (arguments.length == 0) return helpCommand();
        int commandIndex = 0;
        boolean offline = false;
        while (commandIndex < arguments.length && arguments[commandIndex].equals("--offline")) {
            offline = true;
            commandIndex++;
        }
        if (commandIndex == arguments.length || isHelp(arguments[commandIndex])) return helpCommand();
        if (isVersion(arguments[commandIndex])) return versionCommand();
        return new Parser(arguments[commandIndex], offline).parse(arguments, commandIndex + 1);
    }

    private static boolean isHelp(String argument) {
        return argument.equals("--help") || argument.equals("-h");
    }

    private static boolean isVersion(String argument) {
        return argument.equals("--version") || argument.equals("-V");
    }

    private static CommandLine helpCommand() {
        return new CommandLine("", null, null, List.of(), true, false, false,
                null, null, null);
    }

    private static CommandLine versionCommand() {
        return new CommandLine("", null, null, List.of(), false, true, false,
                null, null, null);
    }

    private static final class Parser {
        private final String command;
        private final List<String> runArguments = new ArrayList<>();
        private Path projectRoot;
        private String module;
        private String projectName;
        private String template;
        private String packageName;
        private boolean offline;
        private boolean applicationArguments;

        private Parser(String command, boolean offline) {
            this.command = command;
            this.offline = offline;
        }

        private CommandLine parse(String[] arguments, int firstOption) {
            for (int i = firstOption; i < arguments.length; i++) {
                String argument = arguments[i];
                if (applicationArguments) {
                    runArguments.add(argument);
                } else if (argument.equals("--")) {
                    applicationArguments = true;
                } else {
                    i += parseOption(argument, arguments, i);
                }
            }
            return new CommandLine(command, projectRoot, module, List.copyOf(runArguments),
                    false, false, offline, projectName, template, packageName);
        }

        /** Returns the number of consumed value arguments. */
        private int parseOption(String argument, String[] arguments, int index) {
            return switch (argument) {
                case "--project" -> {
                    projectRoot = Path.of(valueAfter(argument, arguments, index))
                            .toAbsolutePath().normalize();
                    yield 1;
                }
                case "--module" -> {
                    module = valueAfter(argument, arguments, index);
                    yield 1;
                }
                case "--offline" -> {
                    offline = true;
                    yield 0;
                }
                case "--template" -> {
                    requireInitOption(argument);
                    template = valueAfter(argument, arguments, index);
                    yield 1;
                }
                case "--package" -> {
                    requireInitOption(argument);
                    packageName = valueAfter(argument, arguments, index);
                    yield 1;
                }
                default -> {
                    if (command.equals("init") && !argument.startsWith("-")) {
                        if (projectName != null) {
                            throw new IllegalArgumentException("init accepts one project name");
                        }
                        projectName = argument;
                        yield 0;
                    }
                    throw new IllegalArgumentException("unknown argument: " + argument);
                }
            };
        }

        private String valueAfter(String option, String[] arguments, int index) {
            if (index + 1 >= arguments.length || arguments[index + 1].equals("--")) {
                throw new IllegalArgumentException(option + " needs a value");
            }
            return arguments[index + 1];
        }

        private void requireInitOption(String option) {
            if (!command.equals("init")) {
                throw new IllegalArgumentException("unknown argument: " + option);
            }
        }
    }
}
