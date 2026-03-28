package com.jot.samples.workspace.api;

import static org.junit.jupiter.api.Assertions.assertEquals;

import com.jot.samples.workspace.domain.Order;
import org.junit.jupiter.api.Test;

@SuppressWarnings("PMD.AtLeastOneConstructor")
class ApiMainTest {

  @Test
  void orderHasExpectedId() {
    final Order order = new Order("A-1", "jot");
    assertEquals("A-1", order.orderId());
  }
}
