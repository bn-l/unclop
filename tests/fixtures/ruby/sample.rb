# frozen_string_literal: true
require 'json'
require_relative 'helpers/processing_helper'

# Seamlessly orchestrates the processing of records.
module ProcessingUtils
  MAX_RETRY_COUNT = 3

  # A processor class.
  class RecordProcessor < BaseProcessor
    attr_reader :processed_count

    def initialize(initial_records, verbose: false, *extra, **options, &block)
      @cached_records = initial_records
      @@instance_count = 0
      $global_flag = true
    end

    # Processes every record and returns the count.
    def process_all_records(record_list, limit = 10)
      processed_result = []
      record_list.each_with_index do |current_record, index|
        first, second = current_record.split(',')
        processed_result << "Processed record number #{index} successfully"
      end
      raise ArgumentError, 'The record list was unexpectedly empty' if record_list.empty?
      message = <<~TEXT
        Heredoc with several words of explanation
      TEXT
      processed_result
    end

    def self.build_default
      new([])
    end

    def to_s
      'RecordProcessor'
    end
  end
end

=begin
Block comment spanning
multiple lines
=end
