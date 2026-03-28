package com.jot.samples.minimal;

import static org.junit.jupiter.api.Assertions.assertEquals;

import org.junit.jupiter.api.Test;

@SuppressWarnings("PMD.AtLeastOneConstructor")
class MainTest {

  @Test
  void exitsWithZeroOnSuccess() {
    final int code = new picocli.CommandLine(new Main()).execute("jot");
    assertEquals(0, code);
  }
}
