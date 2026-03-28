package com.jot.samples.webserver;

import static org.junit.jupiter.api.Assertions.assertEquals;

import com.jot.samples.webserver.model.Car;
import com.jot.samples.webserver.model.CarDto;
import com.jot.samples.webserver.model.CarMapper;
import org.junit.jupiter.api.Test;

@SuppressWarnings("PMD.AtLeastOneConstructor")
class ServerMainTest {

  @Test
  void carMappedToDto() {
    final Car car = new Car("Toyota", 5, Car.CarType.SEDAN);
    final CarDto dto = CarMapper.INSTANCE.carToCarDto(car);
    assertEquals("Toyota", dto.getMake());
    assertEquals(5, dto.getSeatCount());
    assertEquals("SEDAN", dto.getType());
  }
}
