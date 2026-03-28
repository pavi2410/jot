package com.jot.samples.workspace.cli;

import static org.junit.jupiter.api.Assertions.assertEquals;

import org.junit.jupiter.api.Test;

@SuppressWarnings("PMD.AtLeastOneConstructor")
class CliMainTest {

  @Test
  void simpleMath() {
    assertEquals(4, 2 + 2);
  }
}
