package com.jot.samples.ap;

import com.jot.samples.ap.dto.Car;
import com.jot.samples.ap.dto.Car.CarType;
import com.jot.samples.ap.dto.CarDto;
import com.jot.samples.ap.mapper.CarMapper;

public class Main {
    public static void main(String[] args) {
        Car car = new Car("Tesla", 5, CarType.SEDAN);
        CarDto dto = CarMapper.INSTANCE.carToCarDto(car);
        System.out.println("Source:  " + car.getMake() + " (" + car.getNumberOfSeats() + " seats)");
        System.out.println("Mapped:  " + dto);
    }
}
