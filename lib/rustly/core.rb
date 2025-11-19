# frozen_string_literal: true

require_relative "core/version"
require "rb_sys"
require "json"
require "rustly_core/rustly_core"

# Rustly::Core provides a Ruby wrapper around the native rustly-core extension.
module Rustly
  module Core
    DEFAULT_OPTIONS = {
      strict: false,
      extra: :forbid,
      input_mode: :auto,
      freeze: :none,
      store_attributes: false
    }.freeze

    class << self
      alias native_compile compile unless method_defined?(:native_compile)
      alias native_build build unless method_defined?(:native_build)

      def compile(schema_ast, opts = {})
        normalized = DEFAULT_OPTIONS.merge(symbolize_keys(opts))
        native_compile(schema_ast, normalized)
      end

      def build(compiled_schema, input, klass = Object)
        native_build(compiled_schema, input, klass)
      end

      private

      def symbolize_keys(hash)
        return {} unless hash

        hash.each_with_object({}) do |(key, value), acc|
          sym_key = key.respond_to?(:to_sym) ? key.to_sym : key
          acc[sym_key] = value
        end
      end
    end

    class ValidationError < StandardError
      attr_reader :errors

      def initialize(errors)
        @errors = errors
        super(JSON.generate(errors.messages))
      end
    end
  end
end
