package model

import "time"

type RequestLog struct {
        ID                uint      `json:"id" gorm:"primaryKey"`
        CreatedAt         time.Time `json:"created_at" gorm:"index"`
        RequestID         string    `json:"request_id" gorm:"size:64;index"`
        TokenID           uint      `json:"token_id" gorm:"index"`
        TokenName         string    `json:"token_name" gorm:"size:128"`
        ChannelID         uint      `json:"channel_id" gorm:"index"`
        ChannelName       string    `json:"channel_name" gorm:"size:128"`
        Model             string    `json:"model" gorm:"size:128;index"`
        UpstreamModel     string    `json:"upstream_model" gorm:"size:128"`
        IsStream          bool      `json:"is_stream"`
        DurationMs        int64     `json:"duration_ms"`
        PromptTokens      int       `json:"prompt_tokens"`
        CompletionTokens  int       `json:"completion_tokens"`
        CacheReadTokens   int       `json:"cache_read_tokens"`
        CacheWriteTokens  int       `json:"cache_write_tokens"`
        TotalTokens       int       `json:"total_tokens"`
        CostRMB           float64   `json:"cost_rmb"` // CNY
        Status            string    `json:"status" gorm:"size:32;index"` // success | error
        ErrorMessage      string    `json:"error_message" gorm:"type:text"`
        IP                string    `json:"ip" gorm:"size:64"`
        RequestBody       string    `json:"request_body" gorm:"type:text"`
        ResponseBody      string    `json:"response_body" gorm:"type:text"`
        Detail            string    `json:"detail" gorm:"type:text"` // JSON
}

func (RequestLog) TableName() string {
        return "logs"
}
