package dev.jot.core;

/** Checks that the JDK running Jot can compile and run the configured Java release. */
final class JavaToolchain {
    private JavaToolchain() {}

    static void requireCompatible(String requestedRelease) {
        requireCompatible(requestedRelease, Runtime.version().feature());
    }

    static void requireCompatible(String requestedRelease, int runtimeFeature) {
        int requested;
        try {
            requested = Integer.parseInt(requestedRelease);
        } catch (NumberFormatException error) {
            throw new IllegalArgumentException("toolchains.java must be a Java release number: "
                    + requestedRelease, error);
        }
        if (requested < 1) {
            throw new IllegalArgumentException("toolchains.java must be a positive Java release: "
                    + requestedRelease);
        }
        if (runtimeFeature < requested) {
            throw new IllegalStateException("project requires Java " + requested
                    + ", but Jot is running on Java " + runtimeFeature
                    + "; run Jot with JDK " + requested + " or newer");
        }
    }
}
