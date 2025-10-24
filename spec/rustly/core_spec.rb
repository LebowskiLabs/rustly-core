# frozen_string_literal: true

describe Rustly::Core do
  subject(:core) { described_class }

  let(:schema_ast) do
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
    it "creates a Rustly::Core::CompiledSchema typed object" do
      compiled = core.compile(schema_ast)
      expect(compiled).to be_a(Rustly::Core::CompiledSchema)
      expect(compiled.summary).to include("email")
    end
  end

  describe ".build" do
    before do
      core.compile(schema_ast) # ensure module load
    end

    let(:compiled) { core.compile(schema_ast) }

    it "returns [true, object] when validation succeeds" do
      result = core.build(compiled, { email: "user@example.com" }, {}, Struct.new(:email))
      expect(result).to be_an(Array)
      expect(result.first).to be(true)
      instance = result.last
      expect(instance).to be_a(Struct)
      expect(instance.instance_variable_get(:@attributes)).to eq({ email: "user@example.com" })
      expect(instance).to be_frozen
    end

    it "returns [false, ErrorSet] when validation fails" do
      result = core.build(compiled, { fail: true }, {}, Struct.new(:email))
      expect(result.first).to be(false)
      error_set = result.last
      expect(error_set).to be_a(Rustly::Core::ErrorSet)
      expect(error_set.messages).to all(be_a(String))
      expect(error_set.to_a).to eq(error_set.messages)
    end
  end

  describe "Ruby wrapper" do
    let(:compiled) { core.compile(schema_ast) }

    it "merges options with DEFAULT_BUILD_OPTIONS" do
      expect(core).to receive(:native_build)
        .with(compiled, {}, { strict: false, extra: :allow }, Struct)
        .and_return([true, :ok])

      result = core.build(compiled, {}, { "strict" => false, extra: :allow }, Struct)
      expect(result).to eq([true, :ok])
    end
  end
end
