package relay

import (
        "bufio"
        "context"
        "encoding/json"
        "io"
        "strconv"
        "strings"
        "time"

        "github.com/gofiber/fiber/v2"
        "github.com/newnew/gateway/internal/db"
        "github.com/newnew/gateway/internal/dto"
        "github.com/newnew/gateway/internal/middleware"
        "github.com/newnew/gateway/internal/model"
        claudeup "github.com/newnew/gateway/internal/relay/claude"
        openaiup "github.com/newnew/gateway/internal/relay/openai"
        "github.com/newnew/gateway/internal/service/billing"
        chsvc "github.com/newnew/gateway/internal/service/channel"
        "github.com/newnew/gateway/internal/service/convert"
        logsvc "github.com/newnew/gateway/internal/service/log"
)

const (
        FormatOpenAI = "openai"
        FormatClaude = "claude"
)

type Handler struct {
        openai *openaiup.Client
        claude *claudeup.Client
}

func NewHandler() *Handler {
        timeout := 300
        if v := db.GetSetting(model.SettingRequestTimeout); v != "" {
                if n, err := strconv.Atoi(v); err == nil && n > 0 {
                        timeout = n
                }
        }
        return &Handler{
                openai: openaiup.New(timeout),
                claude: claudeup.New(timeout),
        }
}

func (h *Handler) ChatCompletions(c *fiber.Ctx) error {
        return h.relay(c, FormatOpenAI)
}

func (h *Handler) Messages(c *fiber.Ctx) error {
        return h.relay(c, FormatClaude)
}

func (h *Handler) ListModels(c *fiber.Ctx) error {
        models := chsvc.ListEnabledModels()
        data := make([]dto.ModelItem, 0, len(models))
        now := time.Now().Unix()
        for _, m := range models {
                data = append(data, dto.ModelItem{
                        ID:      m,
                        Object:  "model",
                        Created: now,
                        OwnedBy: "gateway",
                })
        }
        return c.JSON(dto.ModelsListResponse{Object: "list", Data: data})
}

func (h *Handler) relay(c *fiber.Ctx, clientFormat string) error {
        start := time.Now()
        requestID := middleware.GetRequestID(c)
        tok := middleware.GetToken(c)
        rawBody := c.Body()
        bodyStr := string(rawBody)

        modelName, stream, err := peekModelStream(rawBody)
        if err != nil {
                return c.Status(fiber.StatusBadRequest).JSON(errJSON(clientFormat, err.Error()))
        }

        if tok != nil && tok.ModelLimits != "" && tok.ModelLimits != "[]" {
                var limits []string
                if json.Unmarshal([]byte(tok.ModelLimits), &limits) == nil && len(limits) > 0 {
                        ok := false
                        for _, m := range limits {
                                if m == modelName {
                                        ok = true
                                        break
                                }
                        }
                        if !ok {
                                return c.Status(fiber.StatusForbidden).JSON(errJSON(clientFormat, "model not allowed for this token"))
                        }
                }
        }

        ch, err := chsvc.Select(modelName)
        if err != nil {
                return c.Status(fiber.StatusServiceUnavailable).JSON(errJSON(clientFormat, err.Error()))
        }

        upstreamModel := ch.MapModel(modelName)

        price, priceFound := ch.GetModelPrice(modelName)
        if !priceFound {
                price, priceFound = ch.GetModelPrice(upstreamModel)
        }
        if !priceFound && db.GetSetting(model.SettingPriceMissingPolicy) == model.PricePolicyReject {
                return c.Status(fiber.StatusBadRequest).JSON(errJSON(clientFormat, "model price not configured on channel"))
        }

        upstreamBody, err := prepareUpstreamBody(rawBody, clientFormat, ch.Type, upstreamModel)
        if err != nil {
                return c.Status(fiber.StatusBadRequest).JSON(errJSON(clientFormat, "convert request: "+err.Error()))
        }

        apiKey := chsvc.PickKey(ch)

        if stream {
                return h.relayStream(c, clientFormat, ch, tok, modelName, upstreamModel, upstreamBody, apiKey, bodyStr, start, requestID, price, priceFound)
        }

        return h.relayNonStream(c, clientFormat, ch, tok, modelName, upstreamModel, upstreamBody, apiKey, bodyStr, start, requestID, price, priceFound)
}

func (h *Handler) relayNonStream(
        c *fiber.Ctx, clientFormat string, ch *model.Channel, tok *model.Token,
        modelName, upstreamModel string, upstreamBody []byte, apiKey, bodyStr string,
        start time.Time, requestID string, price model.ModelPrice, priceFound bool,
) error {
        ctx := context.Background()
        var (
                usage      billing.Usage
                respBody   string
                status     = "success"
                errMsg     string
                httpStatus = fiber.StatusOK
                clientResp []byte
                err        error
        )

        switch ch.Type {
        case model.ChannelTypeOpenAI:
                resp, e := h.openai.ChatCompletions(ctx, ch.BaseURL, apiKey, upstreamBody, false, ch.FullURL)
                if e != nil {
                        status, errMsg, httpStatus = "error", e.Error(), fiber.StatusBadGateway
                        h.writeLog(tok, ch, modelName, upstreamModel, false, start, usage, billing.Result{PriceMissing: !priceFound, Price: price}, status, errMsg, c.IP(), bodyStr, "", requestID)
                        return c.Status(httpStatus).JSON(errJSON(clientFormat, errMsg))
                }
                raw, _ := io.ReadAll(resp.Body)
                resp.Body.Close()
                respBody = string(raw)
                if resp.StatusCode >= 400 {
                        status, errMsg, httpStatus = "error", openaiup.PrettyUpstreamError(raw), resp.StatusCode
                        h.writeLog(tok, ch, modelName, upstreamModel, false, start, usage, billing.Result{PriceMissing: !priceFound, Price: price}, status, errMsg, c.IP(), bodyStr, respBody, requestID)
                        return c.Status(httpStatus).Type("json").Send(raw)
                }
                usage = extractOpenAIUsage(raw)
                clientResp, err = convertNonStreamResponse(raw, ch.Type, clientFormat, modelName)
        case model.ChannelTypeClaude:
                resp, e := h.claude.Messages(ctx, ch.BaseURL, apiKey, upstreamBody, false, ch.FullURL)
                if e != nil {
                        status, errMsg, httpStatus = "error", e.Error(), fiber.StatusBadGateway
                        h.writeLog(tok, ch, modelName, upstreamModel, false, start, usage, billing.Result{PriceMissing: !priceFound, Price: price}, status, errMsg, c.IP(), bodyStr, "", requestID)
                        return c.Status(httpStatus).JSON(errJSON(clientFormat, errMsg))
                }
                raw, _ := io.ReadAll(resp.Body)
                resp.Body.Close()
                respBody = string(raw)
                if resp.StatusCode >= 400 {
                        status, errMsg, httpStatus = "error", claudeup.PrettyError(raw), resp.StatusCode
                        h.writeLog(tok, ch, modelName, upstreamModel, false, start, usage, billing.Result{PriceMissing: !priceFound, Price: price}, status, errMsg, c.IP(), bodyStr, respBody, requestID)
                        return c.Status(httpStatus).Type("json").Send(raw)
                }
                usage = extractClaudeUsage(raw)
                clientResp, err = convertNonStreamResponse(raw, ch.Type, clientFormat, modelName)
        default:
                return c.Status(fiber.StatusInternalServerError).JSON(errJSON(clientFormat, "unknown channel type"))
        }

        if err != nil {
                status, errMsg = "error", err.Error()
                h.writeLog(tok, ch, modelName, upstreamModel, false, start, usage, billing.Calculate(price, priceFound, usage), status, errMsg, c.IP(), bodyStr, respBody, requestID)
                return c.Status(fiber.StatusInternalServerError).JSON(errJSON(clientFormat, errMsg))
        }

        cost := billing.Calculate(price, priceFound, usage)
        h.writeLog(tok, ch, modelName, upstreamModel, false, start, usage, cost, status, errMsg, c.IP(), bodyStr, string(clientResp), requestID)
        c.Set("Content-Type", "application/json")
        return c.Status(httpStatus).Send(clientResp)
}

func (h *Handler) relayStream(
        c *fiber.Ctx, clientFormat string, ch *model.Channel, tok *model.Token,
        modelName, upstreamModel string, upstreamBody []byte, apiKey, reqBody string,
        start time.Time, requestID string, price model.ModelPrice, priceFound bool,
) error {
        ctx := context.Background()
        var (
                respBody strings.Builder
                usage    billing.Usage
                status   = "success"
                errMsg   string
        )

        c.Set("Content-Type", "text/event-stream")
        c.Set("Cache-Control", "no-cache")
        c.Set("Connection", "keep-alive")
        c.Set("X-Accel-Buffering", "no")

        // Capture IP before stream writer: Fiber/fasthttp context is invalid inside the stream goroutine.
        clientIP := c.IP()

        var sc convert.StreamConverter
        switch {
        case clientFormat == FormatOpenAI && ch.Type == model.ChannelTypeOpenAI:
                sc = convert.NewOpenAIPassthrough()
        case clientFormat == FormatClaude && ch.Type == model.ChannelTypeClaude:
                sc = convert.NewClaudePassthrough()
        case clientFormat == FormatOpenAI && ch.Type == model.ChannelTypeClaude:
                sc = convert.NewClaudeToOpenAIStream(modelName)
        case clientFormat == FormatClaude && ch.Type == model.ChannelTypeOpenAI:
                sc = convert.NewOpenAIToClaudeStream(modelName)
        default:
                sc = convert.NewOpenAIPassthrough()
        }

        c.Context().SetBodyStreamWriter(func(w *bufio.Writer) {
                write := func(b []byte) error {
                        if _, err := w.Write(b); err != nil {
                                return err
                        }
                        return w.Flush()
                }
                defer func() {
                        trailing, u := sc.Finish()
                        if len(trailing) > 0 {
                                _ = write(trailing)
                                respBody.Write(trailing)
                        }
                        if u.PromptTokens > 0 || u.CompletionTokens > 0 {
                                usage = u
                        }
                        cost := billing.Calculate(price, priceFound, usage)
                        h.writeLog(tok, ch, modelName, upstreamModel, true, start, usage, cost, status, errMsg, clientIP, reqBody, respBody.String(), requestID)
                }()

                switch ch.Type {
                case model.ChannelTypeOpenAI:
                        resp, err := h.openai.ChatCompletions(ctx, ch.BaseURL, apiKey, upstreamBody, true, ch.FullURL)
                        if err != nil {
                                status, errMsg = "error", err.Error()
                                b, _ := json.Marshal(errJSON(clientFormat, errMsg))
                                _ = write(b)
                                return
                        }
                        defer resp.Body.Close()
                        if resp.StatusCode >= 400 {
                                raw, _ := io.ReadAll(resp.Body)
                                status, errMsg = "error", openaiup.PrettyUpstreamError(raw)
                                _ = write(raw)
                                return
                        }
                        _ = openaiup.ParseSSELines(resp.Body, func(data string) error {
                                out, err := sc.OnData("", data)
                                if err != nil {
                                        return err
                                }
                                if len(out) == 0 {
                                        return nil
                                }
                                respBody.Write(out)
                                return write(out)
                        })
                case model.ChannelTypeClaude:
                        resp, err := h.claude.Messages(ctx, ch.BaseURL, apiKey, upstreamBody, true, ch.FullURL)
                        if err != nil {
                                status, errMsg = "error", err.Error()
                                b, _ := json.Marshal(errJSON(clientFormat, errMsg))
                                _ = write(b)
                                return
                        }
                        defer resp.Body.Close()
                        if resp.StatusCode >= 400 {
                                raw, _ := io.ReadAll(resp.Body)
                                status, errMsg = "error", claudeup.PrettyError(raw)
                                _ = write(raw)
                                return
                        }
                        _ = claudeup.ParseSSE(resp.Body, func(event, data string) error {
                                out, err := sc.OnData(event, data)
                                if err != nil {
                                        return err
                                }
                                if len(out) == 0 {
                                        return nil
                                }
                                respBody.Write(out)
                                return write(out)
                        })
                }
        })
        return nil
}

func (h *Handler) writeLog(tok *model.Token, ch *model.Channel, modelName, upstreamModel string, stream bool, start time.Time, usage billing.Usage, cost billing.Result, status, errMsg, ip, reqBody, respBody, requestID string) {
        logsvc.Write(logsvc.WriteInput{
                RequestID:     requestID,
                Token:         tok,
                Channel:       ch,
                Model:         modelName,
                UpstreamModel: upstreamModel,
                IsStream:      stream,
                DurationMs:    time.Since(start).Milliseconds(),
                Usage:         usage,
                Cost:          cost,
                Status:        status,
                ErrorMessage:  errMsg,
                IP:            ip,
                RequestBody:   reqBody,
                ResponseBody:  respBody,
        })
}

func peekModelStream(body []byte) (modelName string, stream bool, err error) {
        var m map[string]any
        if err = json.Unmarshal(body, &m); err != nil {
                return "", false, err
        }
        if v, ok := m["model"].(string); ok {
                modelName = v
        }
        if modelName == "" {
                return "", false, fiber.NewError(fiber.StatusBadRequest, "model is required")
        }
        if v, ok := m["stream"].(bool); ok {
                stream = v
        }
        return modelName, stream, nil
}

func prepareUpstreamBody(raw []byte, clientFormat, channelType, upstreamModel string) ([]byte, error) {
        same := (clientFormat == FormatOpenAI && channelType == model.ChannelTypeOpenAI) ||
                (clientFormat == FormatClaude && channelType == model.ChannelTypeClaude)
        if same {
                var m map[string]any
                if err := json.Unmarshal(raw, &m); err != nil {
                        return nil, err
                }
                m["model"] = upstreamModel
                if channelType == model.ChannelTypeOpenAI {
                        if s, _ := m["stream"].(bool); s {
                                m["stream_options"] = map[string]any{"include_usage": true}
                        }
                }
                return json.Marshal(m)
        }

        if clientFormat == FormatOpenAI && channelType == model.ChannelTypeClaude {
                var req dto.OpenAIChatRequest
                if err := json.Unmarshal(raw, &req); err != nil {
                        return nil, err
                }
                req.Model = upstreamModel
                claudeReq, err := convert.OpenAIChatToClaude(&req)
                if err != nil {
                        return nil, err
                }
                return json.Marshal(claudeReq)
        }

        if clientFormat == FormatClaude && channelType == model.ChannelTypeOpenAI {
                var req dto.ClaudeRequest
                if err := json.Unmarshal(raw, &req); err != nil {
                        return nil, err
                }
                req.Model = upstreamModel
                oai, err := convert.ClaudeToOpenAIChat(&req)
                if err != nil {
                        return nil, err
                }
                return json.Marshal(oai)
        }
        return raw, nil
}

func convertNonStreamResponse(raw []byte, channelType, clientFormat, requestModel string) ([]byte, error) {
        same := (clientFormat == FormatOpenAI && channelType == model.ChannelTypeOpenAI) ||
                (clientFormat == FormatClaude && channelType == model.ChannelTypeClaude)
        if same {
                var m map[string]any
                if err := json.Unmarshal(raw, &m); err == nil {
                        m["model"] = requestModel
                        return json.Marshal(m)
                }
                return raw, nil
        }
        if clientFormat == FormatOpenAI && channelType == model.ChannelTypeClaude {
                var resp dto.ClaudeResponse
                if err := json.Unmarshal(raw, &resp); err != nil {
                        return nil, err
                }
                oai := convert.ClaudeResponseToOpenAI(&resp, requestModel)
                return json.Marshal(oai)
        }
        if clientFormat == FormatClaude && channelType == model.ChannelTypeOpenAI {
                var resp dto.OpenAIChatResponse
                if err := json.Unmarshal(raw, &resp); err != nil {
                        return nil, err
                }
                cl := convert.OpenAIResponseToClaude(&resp)
                return json.Marshal(cl)
        }
        return raw, nil
}

func extractOpenAIUsage(raw []byte) billing.Usage {
        var resp dto.OpenAIChatResponse
        if err := json.Unmarshal(raw, &resp); err != nil || resp.Usage == nil {
                return billing.Usage{}
        }
        u := billing.Usage{
                PromptTokens:     resp.Usage.PromptTokens,
                CompletionTokens: resp.Usage.CompletionTokens,
        }
        if resp.Usage.PromptTokensDetails != nil {
                u.CacheReadTokens = resp.Usage.PromptTokensDetails.CachedTokens
        }
        return u
}

func extractClaudeUsage(raw []byte) billing.Usage {
        var resp dto.ClaudeResponse
        if err := json.Unmarshal(raw, &resp); err != nil {
                return billing.Usage{}
        }
        return billing.Usage{
                PromptTokens:     resp.Usage.InputTokens + resp.Usage.CacheReadInputTokens + resp.Usage.CacheCreationInputTokens,
                CompletionTokens: resp.Usage.OutputTokens,
                CacheReadTokens:  resp.Usage.CacheReadInputTokens,
                CacheWriteTokens: resp.Usage.CacheCreationInputTokens,
        }
}

func errJSON(format, msg string) fiber.Map {
        if format == FormatClaude {
                return fiber.Map{
                        "type": "error",
                        "error": fiber.Map{
                                "type":    "api_error",
                                "message": msg,
                        },
                }
        }
        return fiber.Map{
                "error": fiber.Map{
                        "message": msg,
                        "type":    "api_error",
                },
        }
}
