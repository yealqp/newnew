package model

import (
        "encoding/json"
        "strings"
        "time"
)

type Channel struct {
        ID           uint      `json:"id" gorm:"primaryKey"`
        Name         string    `json:"name" gorm:"size:128;not null"`
        Type         string    `json:"type" gorm:"size:32;not null"` // openai | claude
        BaseURL      string    `json:"base_url" gorm:"size:512;not null"`
        // FullURL: when true, BaseURL is the complete chat/messages endpoint
        // (no automatic /v1/chat/completions or /v1/messages suffix).
        FullURL      bool      `json:"full_url" gorm:"default:false"`
        APIKey       string    `json:"api_key" gorm:"type:text;not null"`
        Models       string    `json:"models" gorm:"type:text"` // comma-separated
        ModelMapping string    `json:"model_mapping" gorm:"type:text"` // JSON object
        Status       int       `json:"status" gorm:"default:1"`
        Weight       uint      `json:"weight" gorm:"default:1"`
        Priority     int64     `json:"priority" gorm:"default:0"`
        Pricing      string    `json:"pricing" gorm:"type:text"` // JSON: model -> prices CNY/1M
        Remark       string    `json:"remark" gorm:"size:255"`
        ResponseTime int       `json:"response_time" gorm:"default:0"` // last test latency ms
        TestTime     int64     `json:"test_time" gorm:"default:0"`     // last test unix ts
        CreatedAt    time.Time `json:"created_at"`
        UpdatedAt    time.Time `json:"updated_at"`
}

const (
        ChannelTypeOpenAI = "openai"
        ChannelTypeClaude = "claude"
        ChannelStatusDisabled = 0
        ChannelStatusEnabled  = 1
)

// ModelPrice unit: CNY per 1M tokens
type ModelPrice struct {
        Input      float64 `json:"input"`
        Output     float64 `json:"output"`
        CacheRead  float64 `json:"cache_read"`
        CacheWrite float64 `json:"cache_write"`
}

func (c *Channel) GetModels() []string {
        if c.Models == "" {
                return nil
        }
        parts := strings.Split(c.Models, ",")
        out := make([]string, 0, len(parts))
        for _, p := range parts {
                p = strings.TrimSpace(p)
                if p != "" {
                        out = append(out, p)
                }
        }
        return out
}

func (c *Channel) SupportsModel(model string) bool {
        for _, m := range c.GetModels() {
                if m == model {
                        return true
                }
        }
        return false
}

func (c *Channel) GetKeys() []string {
        if c.APIKey == "" {
                return nil
        }
        parts := strings.Split(c.APIKey, "\n")
        out := make([]string, 0, len(parts))
        for _, p := range parts {
                p = strings.TrimSpace(p)
                if p != "" {
                        out = append(out, p)
                }
        }
        return out
}

func (c *Channel) GetModelMapping() map[string]string {
        m := map[string]string{}
        if c.ModelMapping == "" {
                return m
        }
        _ = json.Unmarshal([]byte(c.ModelMapping), &m)
        return m
}

func (c *Channel) MapModel(clientModel string) string {
        mapping := c.GetModelMapping()
        if up, ok := mapping[clientModel]; ok && up != "" {
                return up
        }
        return clientModel
}

func (c *Channel) GetPricing() map[string]ModelPrice {
        m := map[string]ModelPrice{}
        if c.Pricing == "" {
                return m
        }
        _ = json.Unmarshal([]byte(c.Pricing), &m)
        return m
}

func (c *Channel) GetModelPrice(model string) (ModelPrice, bool) {
        pricing := c.GetPricing()
        // try exact match on client model first, then any key
        if p, ok := pricing[model]; ok {
                return p, true
        }
        return ModelPrice{}, false
}
