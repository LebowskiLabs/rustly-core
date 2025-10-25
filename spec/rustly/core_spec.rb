# frozen_string_literal: true

describe Rustly::Core do
  def build_errors_json(compiled, input, target_class = Struct.new)
    Rustly::Core.build(compiled, input, target_class)
    raise "expected validation to fail"
  rescue Rustly::Core::ValidationError => e
    JSON.generate(e.errors.messages)
  end

  def expect_json_entry(errors_json, entry)
    expected = stringify_keys(entry)
    parsed = JSON.parse(errors_json)
    match = parsed.find { |item| expected.all? { |key, value| item[key] == value } }
    expect(match).not_to be_nil, "expected JSON to include #{expected}, got #{parsed}"
    expect(JSON.generate(match)).to be_json_as(expected)
  end

  def stringify_keys(value)
    case value
    when Hash
      value.each_with_object({}) { |(k, v), acc| acc[k.to_s] = stringify_keys(v) }
    when Array
      value.map { |item| stringify_keys(item) }
    else
      value
    end
  end

  subject(:core) { described_class }

  let(:simple_schema_ast) do
    {
      type: :struct,
      fields: [[:required, :email, :string, { format: :email }]],
      extra: :forbid
    }
  end

  describe ".version" do
    it "returns the extension version" do
      expect(core.version).to be_a(String)
      expect(core.version).to eq(Rustly::Core::VERSION)
    end
  end

  describe ".compile" do
    let(:simple_struct) { Struct.new(:email) }

    it "creates a Rustly::Core::CompiledSchema typed object" do
      compiled = core.compile(simple_schema_ast)
      expect(compiled).to be_a(Rustly::Core::CompiledSchema)
      expect(compiled.summary).to include("email")
    end

    context "with redefined options" do
      it "merges them with DEFAULT_OPTIONS during compile" do
        expect(core).to receive(:native_compile)
          .with(
            simple_schema_ast,
            {
              strict: false,
              extra: :allow,
              input_mode: :auto,
              freeze: :none,
              store_attributes: false
            }
          )
          .and_return(:compiled_schema)

        result = core.compile(simple_schema_ast, { "strict" => false, extra: :allow })
        expect(result).to eq(:compiled_schema)
      end
    end

    context "when extra keys is allowed" do
      it "allows extra keys when compiled with string extra option" do
        compiled = core.compile(simple_schema_ast, { "extra" => :allow })
        expect do
          core.build(
            compiled,
            { email: "user@example.com", nickname: "buddy" },
            simple_struct
          )
        end.not_to raise_error
      end
    end
  end

  describe ".build" do
    let(:compiled_simple) { core.compile(simple_schema_ast) }
    let(:simple_struct)   { Struct.new(:email) }

    it "returns the materialized instance" do
      instance = core.build(compiled_simple, { email: "user@example.com" }, simple_struct)
      expect(instance.instance_variable_defined?(:@attributes)).to be(false)
      expect(instance).to be_a(simple_struct)
      expect(instance).not_to be_frozen
    end

    context "when no base class provided" do
      it "defaults klass to Object" do
        instance = core.build(compiled_simple, { email: "user@example.com" })
        expect(instance).to be_a(Object)
        expect(instance.instance_variable_get(:@email)).to eq("user@example.com")
      end
    end

    context "when raw attributes is requested" do
      it "populates @attributes when requested" do
        compiled = core.compile(simple_schema_ast, store_attributes: true)
        instance = core.build(compiled, { email: "user@example.com" }, simple_struct)
        expect(JSON.generate(instance.instance_variable_get(:@attributes))).to be_json_as(
          { "email" => "user@example.com" }
        )
      end
    end

    context "when some attributes fail validation" do
      it "raises ValidationError when validation fails" do
        expect do
          core.build(compiled_simple, {}, simple_struct)
        end.to raise_error(Rustly::Core::ValidationError) { |error|
          expect(JSON.generate(error.errors.messages)).to be_json_including(
            [
              {
                "path" => "email",
                "code" => "required_missing",
                "meta" => {}
              }
            ]
          )
        }
      end
    end
  end

  context "with detailed error reporting" do
    let(:error_schema_ast) do
      {
        type: :struct,
        extra: :forbid,
        fields: [
          [:required, :email, :str, { format: :email, min_size: 3, max_size: 50 }],
          [:required, :age, :int, { min: 18, max: 65 }],
          [:required, :rating, :float, { min: 1.5, max: 5.0 }],
          %i[required active bool],
          %i[required role sym],
          %i[required payload any],
          [:optional, :metadata, %i[dict int], { min_size: 1 }],
          [:optional, :tags, %i[list str], { min_size: 1, max_size: 3 }],
          [:optional, :status, [:enum, %i[draft published]]],
          [:optional, :nickname, %i[optional str]]
        ]
      }
    end

    let(:compiled) { core.compile(error_schema_ast, strict: true) }
    let(:compiled_lenient) { core.compile(error_schema_ast, strict: false) }
    let(:target_class) do
      Struct.new(:email, :age, :rating, :active, :role, :payload,
                 :metadata, :tags, :status, :nickname)
    end

    it "reports required_missing for absent fields" do
      errors = build_errors_json(
        compiled, { age: 21, rating: 3.0, active: true, role: :user, payload: {} }, target_class
      )
      expect_json_entry(errors, path: "email", code: "required_missing", meta: {})
    end

    it "reports type_mismatch" do
      errors = build_errors_json(
        compiled,
        { email: "user@example.com", age: "old", rating: 2.0, active: true, role: :user,
          payload: {} },
        target_class
      )
      expect_json_entry(errors, path: "age", code: "type_mismatch", meta: { expected: "int" })
    end

    context "when coercion is impossible" do
      it "reports coerce_failed" do
        errors = build_errors_json(
          compiled_lenient,
          { email: "user@example.com", age: "???", rating: 3.2, active: true, role: :user,
            payload: {}, status: :draft },
          target_class
        )
        expect_json_entry(errors, path: "age", code: "coerce_failed", meta: { from: "str" })
      end
    end

    it "reports value_not_in for enums" do
      errors = build_errors_json(
        compiled,
        { email: "user@example.com", age: 32, rating: 4.0, active: true, role: :admin,
          payload: {}, status: :archived },
        target_class
      )
      expect_json_entry(errors, path: "status", code: "value_not_in", meta: { list: %w[draft published] })
    end

    it "reports length and range violations" do
      errors = build_errors_json(
        compiled,
        { email: "x", age: 10, rating: 10.0, active: true, role: :user,
          payload: {}, tags: [], metadata: {}, status: :draft },
        target_class
      )
      expect_json_entry(errors, path: "email", code: "too_short", meta: { min: 3 })
      expect_json_entry(errors, path: "email", code: "format_invalid", meta: { kind: "email" })
      expect_json_entry(errors, path: "age", code: "too_small", meta: { min: 18 })
      expect_json_entry(errors, path: "rating", code: "too_large", meta: { max: 5.0 })
      expect_json_entry(errors, path: "tags", code: "too_short", meta: { min: 1 })
      expect_json_entry(errors, path: "metadata", code: "too_short", meta: { min: 1 })

      long_errors = build_errors_json(
        compiled,
        { email: "a" * 51, age: 30, rating: 2.0, active: true, role: :user,
          payload: {}, tags: %w[a b c d] },
        target_class
      )
      expect_json_entry(long_errors, path: "email", code: "too_long", meta: { max: 50 })
      expect_json_entry(long_errors, path: "tags", code: "too_long", meta: { max: 3 })
    end

    it "reports format_invalid for emails" do
      errors = build_errors_json(
        compiled,
        { email: "invalid", age: 25, rating: 2.0, active: true, role: :user, payload: {} },
        target_class
      )
      expect_json_entry(errors, path: "email", code: "format_invalid", meta: { kind: "email" })
    end

    it "reports extra_key" do
      errors = build_errors_json(
        compiled,
        { email: "user@example.com", age: 25, rating: 2.0,
          active: true, role: :user, payload: {}, bonus: 1 },
        target_class
      )
      expect_json_entry(errors, path: "bonus", code: "extra_key", meta: { name: "bonus" })
    end

    it "tracks nested paths for dict and list values" do
      errors = build_errors_json(
        compiled,
        { email: "user@example.com", age: 30, rating: 3.0, active: true, role: :user,
          payload: {}, metadata: { flag: "wrong" }, tags: ["ok", 123] },
        target_class
      )
      expect_json_entry(errors, path: "metadata.flag", code: "type_mismatch", meta: { expected: "int" })
      expect_json_entry(errors, path: "tags[1]", code: "type_mismatch", meta: { expected: "str" })
    end

    context "when field is optional" do
      it "accepts nil" do
        instance = described_class.build(
          compiled,
          { email: "user@example.com", age: 40, rating: 2.0, active: true, role: :user,
            payload: {}, nickname: nil },
          target_class
        )
        expect(instance).to be_a(target_class)
        expect(instance.nickname).to be_nil
      end
    end
  end
end
