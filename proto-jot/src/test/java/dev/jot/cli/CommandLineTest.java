package dev.jot.cli;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.util.List;
import org.junit.jupiter.api.Test;

class CommandLineTest {
    @Test
    void separatesJotOptionsFromApplicationArguments() {
        CommandLine parsed = CommandLine.parse(new String[] {
                "run", "--project", "demo", "--module", "app", "--", "--port", "8080"
        });

        assertEquals("run", parsed.command());
        assertEquals("app", parsed.module());
        assertEquals(List.of("--port", "8080"), parsed.runArguments());
        assertTrue(parsed.projectRoot().endsWith("demo"));
    }
}
