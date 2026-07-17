package logsvc

import (
        "encoding/json"
        "time"
        "unicode/utf8"

        "github.com/newnew/gateway/internal/db"
        "github.com/newnew/gateway/internal/model"
        "github.com/newnew/gateway/internal/service/billing"
)

type WriteInput struct {
        RequestID        string
        Token            *model.Token
        Channel          *model.Channel
        Model            string
        UpstreamModel    string
        IsStream         bool
        DurationMs       int64
        FirstTokenMs     int64
        Usage            billing.Usage
        Cost             billing.Result
        Status           string
        ErrorMessage     string
        IP               string
        RequestBody      string
        ResponseBody     string
        Detail           map[string]any
}

func Write(in WriteInput) {
        maxBytes := 65536
        if v := db.GetSetting(model.SettingLogBodyMaxBytes); v != "" {
                var n int
                if _, err := parseInt(v, &n); err == nil && n > 0 {
                        maxBytes = n
                }
        }
        detail := in.Detail
        if detail == nil {
                detail = map[string]any{}
        }
        if in.Cost.PriceMissing {
                detail["price_missing"] = true
        } else {
                detail["price"] = in.Cost.Price
        }
        detailJSON, _ := json.Marshal(detail)

        log := model.RequestLog{
                CreatedAt:        time.Now(),
                RequestID:        in.RequestID,
                Model:            in.Model,
                UpstreamModel:    in.UpstreamModel,
                IsStream:         in.IsStream,
                DurationMs:       in.DurationMs,
                FirstTokenMs:     in.FirstTokenMs,
                PromptTokens:     in.Usage.PromptTokens,
                CompletionTokens: in.Usage.CompletionTokens,
                CacheReadTokens:  in.Usage.CacheReadTokens,
                CacheWriteTokens: in.Usage.CacheWriteTokens,
                TotalTokens:      in.Usage.PromptTokens + in.Usage.CompletionTokens,
                CostRMB:          in.Cost.CostRMB,
                Status:           in.Status,
                ErrorMessage:     in.ErrorMessage,
                IP:               in.IP,
                RequestBody:      truncate(in.RequestBody, maxBytes),
                ResponseBody:     truncate(in.ResponseBody, maxBytes),
                Detail:           string(detailJSON),
        }
        if in.Token != nil {
                log.TokenID = in.Token.ID
                log.TokenName = in.Token.Name
        }
        if in.Channel != nil {
                log.ChannelID = in.Channel.ID
                log.ChannelName = in.Channel.Name
        }
        _ = db.DB.Create(&log).Error
}

func truncate(s string, max int) string {
        if max <= 0 || len(s) <= max {
                return s
        }
        // avoid cutting mid-rune
        for max > 0 && !utf8.ValidString(s[:max]) {
                max--
        }
        return s[:max] + "...(truncated)"
}

func parseInt(s string, n *int) (bool, error) {
        var v int
        for _, c := range s {
                if c < '0' || c > '9' {
                        return false, nil
                }
                v = v*10 + int(c-'0')
        }
        *n = v
        return true, nil
}
