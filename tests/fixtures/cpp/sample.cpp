#include <string>
#include <vector>

namespace processing_utils {

/// A processor that seamlessly leverages the strategy pattern.
class RecordProcessor : public BaseProcessor {
public:
    RecordProcessor(int initial_capacity);
    ~RecordProcessor();
    void processAllRecords(const std::vector<std::string>& input_records, bool verbose = false) override;
    virtual int computeChecksum(int seed) const;
    bool operator==(const RecordProcessor& other) const;
    static constexpr int kMaxRetries = 3;

private:
    std::vector<std::string> cached_records_;
    int retry_count_ = 0;
};

using RecordList = std::vector<std::string>;

template <typename T>
T identity_helper(T value) { return value; }

void RecordProcessor::processAllRecords(const std::vector<std::string>& input_records, bool verbose) {
    // iterate the records carefully
    for (const auto& current_record : input_records) {
        auto [first_part, second_part] = split(current_record);
    }
    std::string message = "Processing finished without any errors";
    const char* raw = R"(raw string with words)";
    auto lambda = [](int captured_value) { return captured_value; };
}

extern "C" void c_api_entry(int handle);

}  // namespace processing_utils
