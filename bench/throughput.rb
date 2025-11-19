# frozen_string_literal: true

require "json"
require "benchmark"
require "optparse"

require_relative "../lib/rustly/core"

options = {
  count: (ENV["COUNT"] || 100_000).to_i,
  rounds: (ENV["ROUNDS"] || 3).to_i,
  mode: :ruby,
  json: ENV.fetch("JSON", "1") != "0"
}

OptionParser.new do |parser|
  parser.banner = "Usage: bundle exec ruby bench/throughput.rb [options]"

  parser.on("-c", "--count N", Integer, "Number of records (default: #{options[:count]})") do |count|
    options[:count] = count
  end

  parser.on("-r", "--rounds N", Integer, "Number of measurement rounds (default: #{options[:rounds]})") do |rounds|
    options[:rounds] = rounds
  end

  parser.on("--[no-]json", "Include JSON input benchmarks (default: #{options[:json] ? 'on' : 'off'})") do |flag|
    options[:json] = flag
  end
end.parse!(ARGV)

schema_ast = {
  type: :dict,
  values: {
    type: :struct,
    extra: :forbid,
    fields: [
      [:required, :email, :str, { format: :email, min_size: 10, max_size: 40 }],
      [:required, :age, :int, { min: 18, max: 90 }],
      [:required, :rating, :float, { min: 0.0, max: 10_000.0 }],
      %i[optional active bool],
      [:optional, :tags, %i[list str], { min_size: 1, max_size: 5 }],
      [:optional, :metadata, %i[dict int], { min_size: 1, max_size: 5 }]
    ]
  },
  constraints: { min_size: 1 }
}
# rubocop:disable Metrics/MethodLength
def generate_dataset(count)
  Array.new(count) do |index|
    key = format("user_%06d", index).to_sym
    value = {
      email: format("user%06d@example.com", index),
      age: 18 + (index % 60),
      rating: ((index % 10_000) / 10.0),
      active: index.nobits?(1),
      tags: ["plan_#{index % 5}", "cohort_#{(index / 10) % 5}"],
      metadata: {
        score: index % 1_000,
        level: (index % 50)
      }
    }
    [key, value]
  end.to_h.freeze
end
# rubocop:enable Metrics/MethodLength

def assert_success(tuple, label)
  success, payload = tuple
  return if success

  details = if payload.respond_to?(:messages)
              payload.messages
            else
              payload
            end
  raise "#{label} failed: #{details.inspect}"
end

compiled = Rustly::Core.compile(schema_ast, Rustly::Core::DEFAULT_OPTIONS)
input = generate_dataset(options[:count])
json_input = JSON.generate(input) if options[:json]
bench_opts = Rustly::Core::DEFAULT_OPTIONS

assert_success Rustly::Core::Bench.prepare(input, :ruby), "warmup prepare"
assert_success Rustly::Core::Bench.validate(compiled, input, bench_opts), "warmup validate"
assert_success Rustly::Core::Bench.prepare(json_input, :json), "warmup json prepare" if options[:json]

# rubocop:disable Metrics/MethodLength
def measure(label, rounds)
  times = Array.new(rounds) do
    start = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    yield
    Process.clock_gettime(Process::CLOCK_MONOTONIC) - start
  end
  avg = times.sum / times.size
  min = times.min
  max = times.max
  puts format(
    "%-32<name>s avg: %6.3<avg>fs | min: %6.3<min>fs | max: %6.3<max>fs",
    name: label,
    avg: avg,
    min: min,
    max: max
  )
end
# rubocop:enable Metrics/MethodLength

puts "Rustly::Core throughput benchmark"
puts "Records: #{options[:count]} | Rounds: #{options[:rounds]}"
puts "Dataset size (Hash keys): #{input.size}"
puts

measure("prepare_input(:ruby)", options[:rounds]) do
  assert_success Rustly::Core::Bench.prepare(input, :ruby), "prepare(:ruby)"
end

if options[:json]
  measure("prepare_input(:json)", options[:rounds]) do
    assert_success Rustly::Core::Bench.prepare(json_input, :json), "prepare(:json)"
  end
end

measure("validate_no_gvl", options[:rounds]) do
  assert_success Rustly::Core::Bench.validate(compiled, input, bench_opts), "validate"
end
