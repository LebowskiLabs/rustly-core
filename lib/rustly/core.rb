# frozen_string_literal: true

require_relative "core/version"
require "rb_sys"
require "rustly_core/rustly_core"

# Rustly::Core provides a Ruby wrapper around the native rustly-core extension.
module Rustly
  module Core
    DEFAULT_BUILD_OPTIONS = {
      strict: true,
      extra: :forbid
    }.freeze

    class << self
      alias native_build build unless method_defined?(:native_build)

      def build(compiled_schema, input, opts = {}, klass = Object)
        normalized = DEFAULT_BUILD_OPTIONS.merge(symbolize_keys(opts))
        native_build(compiled_schema, input, normalized, klass)
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
  end
end
