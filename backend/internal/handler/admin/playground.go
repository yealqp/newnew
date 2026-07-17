package admin

import (
	"bufio"
	"context"
	"encoding/json"
	"io"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/newnew/gateway/internal/db"
	"github.com/newnew/gateway/internal/middleware"
	"github.com/newnew/gateway/internal/model"
	openaiup "github.com/newnew/gateway/internal/relay/openai"
	"github.com/newnew/gateway/internal/service/billing"
	"github.com/newnew/gateway/internal/service/channel"
)

type playgroundMsg struct {
	Role    string `json:"role"`
	Content string `json:"content"`
}

type playgroundChatReq struct {
	ConversationID uint            `json:"conversation_id"`
	Model          string          `json:"model"`
	Messages       []playgroundMsg `json:"messages"`
	Temperature    float64         `json:"temperature"`
	MaxTokens      int             `json:"max_tokens"`
}

// ---- Conversation CRUD ----

func ListConversations(c *fiber.Ctx) error {
	var list []struct {
		model.Conversation
		MessageCount int `json:"message_count"`
	}
	if err := db.DB.Model(&model.Conversation{}).
		Select("conversations.*, (SELECT COUNT(*) FROM conversation_messages WHERE conversation_id = conversations.id) AS message_count").
		Order("updated_at desc").
		Find(&list).Error; err != nil {
		return c.Status(500).JSON(fail(err.Error()))
	}
	if list == nil {
		list = []struct {
			model.Conversation
			MessageCount int `json:"message_count"`
		}{}
	}
	return c.JSON(ok(list))
}

func CreateConversation(c *fiber.Ctx) error {
	var req struct {
		Title string `json:"title"`
		Model string `json:"model"`
	}
	if err := c.BodyParser(&req); err != nil {
		return c.Status(400).JSON(fail("invalid body"))
	}
	conv := model.Conversation{
		Title: req.Title,
		Model: req.Model,
	}
	if conv.Title == "" {
		conv.Title = "新对话"
	}
	if err := db.DB.Create(&conv).Error; err != nil {
		return c.Status(500).JSON(fail(err.Error()))
	}
	return c.JSON(ok(conv))
}

func UpdateConversation(c *fiber.Ctx) error {
	id, _ := strconv.Atoi(c.Params("id"))
	var req struct {
		Title string `json:"title"`
		Model string `json:"model"`
	}
	if err := c.BodyParser(&req); err != nil {
		return c.Status(400).JSON(fail("invalid body"))
	}
	if err := db.DB.Model(&model.Conversation{}).Where("id = ?", id).Updates(map[string]any{
		"title":      req.Title,
		"model":      req.Model,
		"updated_at": time.Now(),
	}).Error; err != nil {
		return c.Status(500).JSON(fail(err.Error()))
	}
	return c.JSON(ok(nil))
}

func DeleteConversation(c *fiber.Ctx) error {
	id, _ := strconv.Atoi(c.Params("id"))
	tx := db.DB.Begin()
	tx.Delete(&model.ConversationMessage{}, "conversation_id = ?", id)
	tx.Delete(&model.Conversation{}, id)
	if err := tx.Commit().Error; err != nil {
		return c.Status(500).JSON(fail(err.Error()))
	}
	return c.JSON(ok(nil))
}

// ---- Messages ----

func ListMessages(c *fiber.Ctx) error {
	id, _ := strconv.Atoi(c.Params("id"))
	var msgs []model.ConversationMessage
	if err := db.DB.Where("conversation_id = ?", id).Order("id asc").Find(&msgs).Error; err != nil {
		return c.Status(500).JSON(fail(err.Error()))
	}
	if msgs == nil {
		msgs = []model.ConversationMessage{}
	}
	return c.JSON(ok(msgs))
}

func ClearMessages(c *fiber.Ctx) error {
	id, _ := strconv.Atoi(c.Params("id"))
	if err := db.DB.Delete(&model.ConversationMessage{}, "conversation_id = ?", id).Error; err != nil {
		return c.Status(500).JSON(fail(err.Error()))
	}
	return c.JSON(ok(nil))
}

// ---- Chat ----

func PlaygroundChat(c *fiber.Ctx) error {
	start := time.Now()
	var req playgroundChatReq
	if err := c.BodyParser(&req); err != nil {
		return c.Status(400).JSON(fail("invalid body"))
	}
	if req.Model == "" || len(req.Messages) == 0 {
		return c.Status(400).JSON(fail("model and messages required"))
	}
	if req.ConversationID == 0 {
		return c.Status(400).JSON(fail("conversation_id required"))
	}

	// Verify conversation exists
	var conv model.Conversation
	if err := db.DB.First(&conv, req.ConversationID).Error; err != nil {
		return c.Status(404).JSON(fail("conversation not found"))
	}

	requestID := middleware.GetRequestID(c)

	// Save the user message
	lastMsg := req.Messages[len(req.Messages)-1]
	userMsg := model.ConversationMessage{
		ConversationID: req.ConversationID,
		Role:           lastMsg.Role,
		Content:        lastMsg.Content,
	}
	db.DB.Create(&userMsg)

	ch, err := channel.Select(req.Model)
	if err != nil {
		db.DB.Model(&conv).Update("updated_at", time.Now())
		return c.Status(503).JSON(fail(err.Error()))
	}

	upstreamModel := ch.MapModel(req.Model)
	apiKey := channel.PickKey(ch)

	// Build upstream body
	bodyMap := map[string]any{
		"model":    upstreamModel,
		"messages": req.Messages,
		"stream":   true,
	}
	if req.Temperature > 0 {
		bodyMap["temperature"] = req.Temperature
	}
	if req.MaxTokens > 0 {
		bodyMap["max_tokens"] = req.MaxTokens
	}
	body, _ := json.Marshal(bodyMap)

	timeout := 120
	if v := db.GetSetting(model.SettingRequestTimeout); v != "" {
		if n, err := strconv.Atoi(v); err == nil && n > 0 {
			timeout = n
		}
	}
	client := openaiup.New(timeout)

	resp, err := client.ChatCompletions(context.Background(), ch.BaseURL, apiKey, body, true, ch.FullURL)
	if err != nil {
		db.DB.Model(&conv).Update("updated_at", time.Now())
		writePlaygroundLog(requestID, ch, req.Model, upstreamModel, start, 0, billing.Usage{}, billing.Result{PriceMissing: true}, "error", err.Error(), c.IP(), string(body), "")
		return c.Status(502).JSON(fail("upstream error: " + err.Error()))
	}

	if resp.StatusCode >= 400 {
		raw, _ := io.ReadAll(resp.Body)
		resp.Body.Close()
		db.DB.Model(&conv).Update("updated_at", time.Now())
		writePlaygroundLog(requestID, ch, req.Model, upstreamModel, start, 0, billing.Usage{}, billing.Result{PriceMissing: true}, "error", "upstream "+strconv.Itoa(resp.StatusCode), c.IP(), string(body), string(raw))
		return c.Status(resp.StatusCode).Type("json").Send(raw)
	}

	// Stream response back while tracking usage
	// Capture IP before stream writer: Fiber/fasthttp ctx is invalid inside the stream goroutine.
	clientIP := c.IP()
	c.Set("Content-Type", "text/event-stream")
	c.Set("Cache-Control", "no-cache")
	c.Set("Connection", "keep-alive")
	c.Set("X-Accel-Buffering", "no")

	c.Context().SetBodyStreamWriter(func(w *bufio.Writer) {
		defer resp.Body.Close()
		write := func(b []byte) error {
			if _, err := w.Write(b); err != nil {
				return err
			}
			return w.Flush()
		}

		var (
			fullContent  string
			respBody     strings.Builder
			finalUsage   billing.Usage
			firstTokenMs int64
			firstOnce    sync.Once
		)

		_ = openaiup.ParseSSELines(resp.Body, func(data string) error {
			if data == "[DONE]" {
				// Save assistant message
				if fullContent != "" {
					db.DB.Create(&model.ConversationMessage{
						ConversationID: req.ConversationID,
						Role:           "assistant",
						Content:        fullContent,
					})
				}
				db.DB.Model(&conv).Updates(map[string]any{
					"updated_at": time.Now(),
				})
				// Auto-title
				if conv.Title == "新对话" {
					title := lastMsg.Content
					if len([]rune(title)) > 40 {
						title = string([]rune(title)[:40]) + "…"
					}
					db.DB.Model(&conv).Update("title", title)
				}

				// Write to request log
				cost := billing.CalculateForChannel(ch, req.Model, finalUsage)
				writePlaygroundLog(requestID, ch, req.Model, upstreamModel, start, firstTokenMs, finalUsage, cost, "success", "", clientIP, string(body), respBody.String())

				return write([]byte("data: [DONE]\n\n"))
			}

			// Track first token time
			firstOnce.Do(func() { firstTokenMs = time.Since(start).Milliseconds() })

			// Track response body for logging
			respBody.WriteString("data: ")
			respBody.WriteString(data)
			respBody.WriteString("\n\n")

			// Extract content delta
			delta, usage := parseStreamChunk(data)
			if delta != "" {
				fullContent += delta
			}
			if usage != nil {
				finalUsage = *usage
			}

			return write([]byte("data: " + data + "\n\n"))
		})
	})
	return nil
}

// parseStreamChunk extracts content delta and optional usage from an SSE data line.
func parseStreamChunk(data string) (content string, usage *billing.Usage) {
	var obj struct {
		Choices []struct {
			Delta struct {
				Content string `json:"content"`
			} `json:"delta"`
		} `json:"choices"`
		Usage *struct {
			PromptTokens     int `json:"prompt_tokens"`
			CompletionTokens int `json:"completion_tokens"`
			TotalTokens      int `json:"total_tokens"`
			PromptTokensDetails *struct {
				CachedTokens int `json:"cached_tokens"`
			} `json:"prompt_tokens_details"`
		} `json:"usage"`
	}
	if err := json.Unmarshal([]byte(data), &obj); err != nil {
		return "", nil
	}
	if len(obj.Choices) > 0 {
		content = obj.Choices[0].Delta.Content
	}
	if obj.Usage != nil {
		u := billing.Usage{
			PromptTokens:     obj.Usage.PromptTokens,
			CompletionTokens: obj.Usage.CompletionTokens,
		}
		if obj.Usage.PromptTokensDetails != nil {
			u.CacheReadTokens = obj.Usage.PromptTokensDetails.CachedTokens
		}
		usage = &u
	}
	return
}

func writePlaygroundLog(requestID string, ch *model.Channel, modelName, upstreamModel string, start time.Time, firstTokenMs int64, usage billing.Usage, cost billing.Result, status, errMsg, ip, reqBody, respBody string) {
	db.DB.Create(&model.RequestLog{
		CreatedAt:        time.Now(),
		RequestID:        requestID,
		TokenName:        "游乐场",
		ChannelID:        ch.ID,
		ChannelName:      ch.Name,
		Model:            modelName,
		UpstreamModel:    upstreamModel,
		IsStream:         true,
		DurationMs:       time.Since(start).Milliseconds(),
		FirstTokenMs:     firstTokenMs,
		PromptTokens:     usage.PromptTokens,
		CompletionTokens: usage.CompletionTokens,
		CacheReadTokens:  usage.CacheReadTokens,
		CacheWriteTokens: usage.CacheWriteTokens,
		TotalTokens:      usage.PromptTokens + usage.CompletionTokens,
		CostRMB:          cost.CostRMB,
		Status:           status,
		ErrorMessage:     errMsg,
		IP:               ip,
		RequestBody:      reqBody,
		ResponseBody:     respBody,
	})
}
