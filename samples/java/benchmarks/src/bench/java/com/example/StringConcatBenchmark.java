package com.example;

import org.openjdk.jmh.annotations.Benchmark;
import org.openjdk.jmh.annotations.BenchmarkMode;
import org.openjdk.jmh.annotations.Mode;
import org.openjdk.jmh.annotations.OutputTimeUnit;
import org.openjdk.jmh.annotations.Scope;
import org.openjdk.jmh.annotations.State;

import java.util.concurrent.TimeUnit;

@State(Scope.Thread)
@BenchmarkMode(Mode.AverageTime)
@OutputTimeUnit(TimeUnit.NANOSECONDS)
public class StringConcatBenchmark {

    @Benchmark
    public String plusConcat() {
        return "Hello" + ", " + "world!";
    }

    @Benchmark
    public String builderConcat() {
        return new StringBuilder()
                .append("Hello")
                .append(", ")
                .append("world!")
                .toString();
    }
}
