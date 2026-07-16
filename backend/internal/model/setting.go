package model

type Setting struct {
        Key   string `json:"key" gorm:"primaryKey;size:128"`
        Value string `json:"value" gorm:"type:text"`
}

const (
        SettingLogBodyMaxBytes    = "log_body_max_bytes"
        SettingPriceMissingPolicy = "price_missing_policy" // allow | reject
        SettingRequestTimeout     = "request_timeout"      // seconds
)

const (
        PricePolicyAllow  = "allow"
        PricePolicyReject = "reject"
)
