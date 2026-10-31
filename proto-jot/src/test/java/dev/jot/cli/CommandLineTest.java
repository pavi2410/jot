package dev.jot.cli;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.nio.file.Path;
import java.util.List;
import org.junit.jupiter.api.Test;

class CommandLineTest {
    @Test
    void separatesJotOptionsFromApplicationArguments() {
        CommandLine parsed = CommandLine.parse(new String[] {
                "run", "--project", "demo", "--module", "app", "--offline", "--", "--port", "8080"
        });

        assertEquals("run", parsed.command());
        assertEquals("app", parsed.module());
        assertEquals(List.of("--port", "8080"), parsed.runArguments());
        assertTrue(parsed.projectRoot().endsWith("demo"));
        assertTrue(parsed.offline());
    }

    @Test
    void acceptsOfflineAsAGlobalOptionBeforeTheCommand() {
        CommandLine parsed = CommandLine.parse(new String[] {"--offline", "build"});

        assertEquals("build", parsed.command());
        assertTrue(parsed.offline());
    }

    @Test
    void errorsWithoutMessagesStillShowTheirType() {
        assertEquals("java.lang.IllegalStateException",
                Main.failureMessage(new IllegalStateException()));
    }

    @Test
    void parsesProjectInitializationOptions() {
        CommandLine parsed = CommandLine.parse(new String[] {
                "init", "sample-app", "--template", "library", "--package", "dev.sample"
        });

        assertEquals("init", parsed.command());
        assertEquals("sample-app", parsed.projectName());
        assertEquals("library", parsed.template());
        assertEquals("dev.sample", parsed.packageName());
    }

    @Test
    void rejectsMissingOptionValuesAndInitOptionsOnOtherCommands() {
        assertEquals("--project needs a value", parseError("build", "--project"));
        assertEquals("unknown argument: --template", parseError("build", "--template", "app"));
    }

    private static String parseError(String... arguments) {
        try {
            CommandLine.parse(arguments);
            throw new AssertionError("expected argument parsing to fail");
        } catch (IllegalArgumentException error) {
            return error.getMessage();
        }
    }

    @Test
    void infersValidProjectNamesFromDirectoryNames() {
        assertEquals("jot-init-smoke_abc123",
                Main.inferredProjectName(Path.of("/tmp/jot-init.smoke_abc123")));
        assertEquals("project-123-demo",
                Main.inferredProjectName(Path.of("/tmp/123.demo")));
    }
}
