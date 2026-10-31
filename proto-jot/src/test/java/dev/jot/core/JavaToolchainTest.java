package dev.jot.core;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;

import org.junit.jupiter.api.Test;

class JavaToolchainTest {
    @Test
    void acceptsAReleaseSupportedByTheRunningJdk() {
        JavaToolchain.requireCompatible("21", 27);
    }

    @Test
    void explainsWhenTheRunningJdkIsTooOld() {
        IllegalStateException error = assertThrows(IllegalStateException.class,
                () -> JavaToolchain.requireCompatible("27", 21));
        assertEquals("project requires Java 27, but Jot is running on Java 21; run Jot with JDK 27 or newer",
                error.getMessage());
    }

    @Test
    void rejectsNonNumericReleaseRequests() {
        IllegalArgumentException error = assertThrows(IllegalArgumentException.class,
                () -> JavaToolchain.requireCompatible("latest", 27));
        assertEquals("toolchains.java must be a Java release number: latest", error.getMessage());
    }
}
