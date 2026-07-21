package admin

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/newnew/gateway/internal/db"
	"github.com/newnew/gateway/internal/middleware"
	"github.com/newnew/gateway/internal/model"
	claudeup "github.com/newnew/gateway/internal/relay/claude"
	openaiup "github.com/newnew/gateway/internal/relay/openai"
	"github.com/newnew/gateway/internal/service/billing"
	"github.com/newnew/gateway/internal/service/channel"
	"golang.org/x/crypto/bcrypt"
	"gorm.io/gorm"
)

func ok(data any) fiber.Map {
	return fiber.Map{"success": true, "data": data}
}

func fail(msg string) fiber.Map {
	return fiber.Map{"success": false, "message": msg}
}

// ---- Auth ----

type loginReq struct {
	Username string `json:"username"`
	Password string `json:"password"`
}

func Login(c *fiber.Ctx) error {
	var req loginReq
	if err := c.BodyParser(&req); err != nil {
		return c.Status(400).JSON(fail("invalid body"))
	}
	var user model.User
	if err := db.DB.Where("username = ?", req.Username).First(&user).Error; err != nil {
		return c.Status(401).JSON(fail("invalid username or password"))
	}
	if bcrypt.CompareHashAndPassword([]byte(user.PasswordHash), []byte(req.Password)) != nil {
		return c.Status(401).JSON(fail("invalid username or password"))
	}
	token, err := middleware.GenerateJWT(user.ID, user.Username)
	if err != nil {
		return c.Status(500).JSON(fail("token error"))
	}
	return c.JSON(ok(fiber.Map{
		"token":    token,
		"username": user.Username,
	}))
}

// ---- Setup (first-run initialization) ----

func SetupStatus(c *fiber.Ctx) error {
	var count int64
	db.DB.Model(&model.User{}).Count(&count)
	return c.JSON(ok(fiber.Map{"initialized": count > 0}))
}

type setupReq struct {
	Username string `json:"username"`
	Password string `json:"password"`
}

func Setup(c *fiber.Ctx) error {
	var count int64
	db.DB.Model(&model.User{}).Count(&count)
	if count > 0 {
		return c.Status(400).JSON(fail("already initialized"))
	}

	var req setupReq
	if err := c.BodyParser(&req); err != nil {
		return c.Status(400).JSON(fail("invalid body"))
	}
	req.Username = strings.TrimSpace(req.Username)
	if len(req.Username) < 3 {
		return c.Status(400).JSON(fail("username too short (min 3 characters)"))
	}
	if len(req.Password) < 6 {
		return c.Status(400).JSON(fail("password too short (min 6 characters)"))
	}

	hash, err := bcrypt.GenerateFromPassword([]byte(req.Password), bcrypt.DefaultCost)
	if err != nil {
		return c.Status(500).JSON(fail("hash error"))
	}
	user := model.User{
		Username:     req.Username,
		PasswordHash: string(hash),
	}
	if err := db.DB.Create(&user).Error; err != nil {
		return c.Status(500).JSON(fail(err.Error()))
	}

	token, err := middleware.GenerateJWT(user.ID, user.Username)
	if err != nil {
		return c.Status(500).JSON(fail("token error"))
	}
	return c.JSON(ok(fiber.Map{
		"token":    token,
		"username": user.Username,
	}))
}

func Me(c *fiber.Ctx) error {
	return c.JSON(ok(fiber.Map{
		"username": c.Locals(middleware.LocalsUsername),
		"id":       c.Locals(middleware.LocalsUserID),
	}))
}

type changePasswordReq struct {
	OldPassword string `json:"old_password"`
	NewUsername string `json:"new_username"`
	NewPassword string `json:"new_password"`
}

// ChangePassword updates the current admin's username and/or password.
// The current password is always required to authorize the change.
func ChangePassword(c *fiber.Ctx) error {
	var req changePasswordReq
	if err := c.BodyParser(&req); err != nil {
		return c.Status(400).JSON(fail("invalid body"))
	}
	uid, _ := c.Locals(middleware.LocalsUserID).(uint)
	var user model.User
	if err := db.DB.First(&user, uid).Error; err != nil {
		return c.Status(404).JSON(fail("user not found"))
	}
	if bcrypt.CompareHashAndPassword([]byte(user.PasswordHash), []byte(req.OldPassword)) != nil {
		return c.Status(400).JSON(fail("old password incorrect"))
	}

	changed := false
	if newName := strings.TrimSpace(req.NewUsername); newName != "" && newName != user.Username {
		if len(newName) < 3 {
			return c.Status(400).JSON(fail("username too short"))
		}
		var cnt int64
		db.DB.Model(&model.User{}).Where("username = ? AND id <> ?", newName, user.ID).Count(&cnt)
		if cnt > 0 {
			return c.Status(400).JSON(fail("username already exists"))
		}
		user.Username = newName
		changed = true
	}
	if req.NewPassword != "" {
		if len(req.NewPassword) < 6 {
			return c.Status(400).JSON(fail("password too short"))
		}
		hash, err := bcrypt.GenerateFromPassword([]byte(req.NewPassword), bcrypt.DefaultCost)
		if err != nil {
			return c.Status(500).JSON(fail("hash error"))
		}
		user.PasswordHash = string(hash)
		changed = true
	}
	if !changed {
		return c.Status(400).JSON(fail("nothing to update"))
	}
	if err := db.DB.Save(&user).Error; err != nil {
		return c.Status(500).JSON(fail(err.Error()))
	}
	// 重新签发 JWT，使会话中的用户名保持最新。
	token, err := middleware.GenerateJWT(user.ID, user.Username)
	if err != nil {
		return c.Status(500).JSON(fail("token error"))
	}
	return c.JSON(ok(fiber.Map{"username": user.Username, "token": token}))
}

// ---- Channels ----

// channelUsage aggregates log statistics per channel.
type channelUsage struct {
	ChannelID        uint
	Total            int64
	Requests         int64
	PromptTokens     int64
	CompletionTokens int64
	CostRMB          float64
}

func ListChannels(c *fiber.Ctx) error {
	var list []model.Channel
	if err := db.DB.Order("id desc").Find(&list).Error; err != nil {
		return c.Status(500).JSON(fail(err.Error()))
	}
	// aggregate total_tokens per channel from logs
	var aggs []channelUsage
	_ = db.DB.Model(&model.RequestLog{}).
		Select("channel_id, count(*) as requests, coalesce(sum(total_tokens),0) as total, coalesce(sum(prompt_tokens),0) as prompt_tokens, coalesce(sum(completion_tokens),0) as completion_tokens, coalesce(sum(cost_rmb),0) as cost_rmb").
		Where("status = ?", "success").
		Group("channel_id").
		Scan(&aggs).Error
	m := map[uint]channelUsage{}
	for _, a := range aggs {
		m[a.ChannelID] = a
	}
	out := make([]fiber.Map, 0, len(list))
	for _, ch := range list {
		a := m[ch.ID]
		out = append(out, channelView(ch, false, a))
	}
	return c.JSON(ok(out))
}

func GetChannel(c *fiber.Ctx) error {
	id, _ := strconv.Atoi(c.Params("id"))
	var ch model.Channel
	if err := db.DB.First(&ch, id).Error; err != nil {
		return c.Status(404).JSON(fail("not found"))
	}
	return c.JSON(ok(channelView(ch, true, channelUsage{})))
}

func CreateChannel(c *fiber.Ctx) error {
	var ch model.Channel
	if err := c.BodyParser(&ch); err != nil {
		return c.Status(400).JSON(fail("invalid body"))
	}
	if ch.Name == "" || ch.Type == "" || ch.BaseURL == "" || ch.APIKey == "" {
		return c.Status(400).JSON(fail("name, type, base_url, api_key required"))
	}
	if ch.Type != model.ChannelTypeOpenAI && ch.Type != model.ChannelTypeClaude {
		return c.Status(400).JSON(fail("type must be openai or claude"))
	}
	if ch.Weight == 0 {
		ch.Weight = 1
	}
	if ch.Status == 0 {
		ch.Status = model.ChannelStatusEnabled
	}
	if err := db.DB.Create(&ch).Error; err != nil {
		return c.Status(500).JSON(fail(err.Error()))
	}
	return c.JSON(ok(channelView(ch, false, channelUsage{})))
}

func UpdateChannel(c *fiber.Ctx) error {
	id, _ := strconv.Atoi(c.Params("id"))
	var ch model.Channel
	if err := db.DB.First(&ch, id).Error; err != nil {
		return c.Status(404).JSON(fail("not found"))
	}
	var req model.Channel
	if err := c.BodyParser(&req); err != nil {
		return c.Status(400).JSON(fail("invalid body"))
	}
	ch.Name = req.Name
	ch.Type = req.Type
	ch.BaseURL = req.BaseURL
	ch.FullURL = req.FullURL
	if req.APIKey != "" && !strings.Contains(req.APIKey, "***") {
		ch.APIKey = req.APIKey
	}
	ch.Models = req.Models
	ch.ModelMapping = req.ModelMapping
	ch.Status = req.Status
	ch.Weight = req.Weight
	ch.Priority = req.Priority
	ch.Pricing = req.Pricing
	ch.Remark = req.Remark
	ch.Icon = req.Icon
	if err := db.DB.Save(&ch).Error; err != nil {
		return c.Status(500).JSON(fail(err.Error()))
	}
	return c.JSON(ok(channelView(ch, false, channelUsage{})))
}

func DeleteChannel(c *fiber.Ctx) error {
	id, _ := strconv.Atoi(c.Params("id"))
	if err := db.DB.Delete(&model.Channel{}, id).Error; err != nil {
		return c.Status(500).JSON(fail(err.Error()))
	}
	return c.JSON(ok(nil))
}

// TestChannel sends a minimal "hi" request to the upstream channel and returns latency.
// Inspired by new-api controller.TestChannel / buildTestRequest.
func TestChannel(c *fiber.Ctx) error {
	id, err := strconv.Atoi(c.Params("id"))
	if err != nil {
		return c.Status(400).JSON(fail("invalid channel id"))
	}
	var ch model.Channel
	if err := db.DB.First(&ch, id).Error; err != nil {
		return c.Status(404).JSON(fail("channel not found"))
	}

	testModel := strings.TrimSpace(c.Query("model"))
	if testModel == "" {
		models := ch.GetModels()
		if len(models) > 0 {
			testModel = models[0]
		}
	}
	if testModel == "" {
		return c.Status(400).JSON(fail("channel has no models to test"))
	}

	upstreamModel := ch.MapModel(testModel)
	apiKey := channel.PickKey(&ch)
	if strings.TrimSpace(apiKey) == "" {
		return c.Status(400).JSON(fail("channel has no api key"))
	}

	timeoutSec := 30
	if v := db.GetSetting(model.SettingRequestTimeout); v != "" {
		if n, e := strconv.Atoi(v); e == nil && n > 0 {
			if n < timeoutSec {
				timeoutSec = n
			}
		}
	}

	maxTokens := 16
	tik := time.Now()
	var (
		statusCode int
		respBody   []byte
		testErr    error
	)

	ctx, cancel := context.WithTimeout(context.Background(), time.Duration(timeoutSec)*time.Second)
	defer cancel()

	switch ch.Type {
	case model.ChannelTypeOpenAI:
		body, _ := json.Marshal(map[string]any{
			"model":      upstreamModel,
			"max_tokens": maxTokens,
			"messages": []map[string]string{
				{"role": "user", "content": "hi"},
			},
			"stream": false,
		})
		client := openaiup.New(timeoutSec)
		resp, e := client.ChatCompletions(ctx, ch.BaseURL, apiKey, body, false, ch.FullURL)
		if e != nil {
			testErr = e
			break
		}
		statusCode = resp.StatusCode
		respBody, _ = io.ReadAll(resp.Body)
		resp.Body.Close()
		if statusCode >= 400 {
			testErr = fmt.Errorf("upstream %d: %s", statusCode, openaiup.PrettyUpstreamError(respBody))
		}
	case model.ChannelTypeClaude:
		body, _ := json.Marshal(map[string]any{
			"model":      upstreamModel,
			"max_tokens": maxTokens,
			"messages": []map[string]string{
				{"role": "user", "content": "hi"},
			},
			"stream": false,
		})
		client := claudeup.New(timeoutSec)
		resp, e := client.Messages(ctx, ch.BaseURL, apiKey, body, false, ch.FullURL)
		if e != nil {
			testErr = e
			break
		}
		statusCode = resp.StatusCode
		respBody, _ = io.ReadAll(resp.Body)
		resp.Body.Close()
		if statusCode >= 400 {
			testErr = fmt.Errorf("upstream %d: %s", statusCode, claudeup.PrettyError(respBody))
		}
	default:
		return c.Status(400).JSON(fail("unsupported channel type"))
	}

	ms := time.Since(tik).Milliseconds()
	now := time.Now().Unix()
	// always record last test time/latency
	_ = db.DB.Model(&model.Channel{}).Where("id = ?", ch.ID).Updates(map[string]any{
		"response_time": ms,
		"test_time":     now,
	}).Error

	usage := extractChannelTestUsage(ch.Type, respBody)
	cost := billing.CalculateForChannel(&ch, testModel, usage)
	testStatus := "success"
	testErrMessage := ""
	if testErr != nil {
		testStatus = "error"
		testErrMessage = testErr.Error()
	}
	db.DB.Create(&model.RequestLog{
		CreatedAt:        time.Now(),
		RequestID:        middleware.GetRequestID(c),
		TokenName:        "测试",
		ChannelID:        ch.ID,
		ChannelName:      ch.Name,
		Model:            testModel,
		UpstreamModel:    upstreamModel,
		IsStream:         false,
		DurationMs:       ms,
		PromptTokens:     usage.PromptTokens,
		CompletionTokens: usage.CompletionTokens,
		CacheReadTokens:  usage.CacheReadTokens,
		CacheWriteTokens: usage.CacheWriteTokens,
		TotalTokens:      usage.PromptTokens + usage.CompletionTokens,
		CostRMB:          cost.CostRMB,
		Status:           testStatus,
		ErrorMessage:     testErrMessage,
		IP:               c.IP(),
		RequestBody:      channelTestRequestBody(ch.Type, upstreamModel, maxTokens),
		ResponseBody:     string(respBody),
	})

	if testErr != nil {
		return c.JSON(fiber.Map{
			"success": false,
			"message": testErr.Error(),
			"data": fiber.Map{
				"channel_id":     ch.ID,
				"model":          testModel,
				"upstream_model": upstreamModel,
				"response_time":  ms,
				"time":           float64(ms) / 1000.0,
				"status_code":    statusCode,
			},
		})
	}

	// optional: truncate body for preview
	preview := string(respBody)
	if len(preview) > 500 {
		preview = preview[:500] + "..."
	}

	return c.JSON(ok(fiber.Map{
		"channel_id":     ch.ID,
		"model":          testModel,
		"upstream_model": upstreamModel,
		"response_time":  ms,
		"time":           float64(ms) / 1000.0,
		"status_code":    statusCode,
		"preview":        preview,
	}))
}

func channelTestRequestBody(channelType, upstreamModel string, maxTokens int) string {
	body, _ := json.Marshal(map[string]any{
		"model":      upstreamModel,
		"max_tokens": maxTokens,
		"messages": []map[string]string{
			{"role": "user", "content": "hi"},
		},
		"stream": false,
		"type":   channelType,
	})
	return string(body)
}

func extractChannelTestUsage(channelType string, raw []byte) billing.Usage {
	if channelType == model.ChannelTypeOpenAI {
		var resp struct {
			Usage *struct {
				PromptTokens     int `json:"prompt_tokens"`
				CompletionTokens int `json:"completion_tokens"`
				PromptDetails    *struct {
					CachedTokens int `json:"cached_tokens"`
				} `json:"prompt_tokens_details"`
			} `json:"usage"`
		}
		if json.Unmarshal(raw, &resp) == nil && resp.Usage != nil {
			usage := billing.Usage{
				PromptTokens:     resp.Usage.PromptTokens,
				CompletionTokens: resp.Usage.CompletionTokens,
			}
			if resp.Usage.PromptDetails != nil {
				usage.CacheReadTokens = resp.Usage.PromptDetails.CachedTokens
			}
			return usage
		}
		return billing.Usage{}
	}

	var resp struct {
		Usage struct {
			InputTokens              int `json:"input_tokens"`
			OutputTokens             int `json:"output_tokens"`
			CacheReadInputTokens     int `json:"cache_read_input_tokens"`
			CacheCreationInputTokens int `json:"cache_creation_input_tokens"`
		} `json:"usage"`
	}
	if json.Unmarshal(raw, &resp) != nil {
		return billing.Usage{}
	}
	return billing.Usage{
		PromptTokens:     resp.Usage.InputTokens + resp.Usage.CacheReadInputTokens + resp.Usage.CacheCreationInputTokens,
		CompletionTokens: resp.Usage.OutputTokens,
		CacheReadTokens:  resp.Usage.CacheReadInputTokens,
		CacheWriteTokens: resp.Usage.CacheCreationInputTokens,
	}
}

type fetchModelsReq struct {
	BaseURL   string `json:"base_url"`
	APIKey    string `json:"api_key"`
	Type      string `json:"type"` // openai | claude
	FullURL   bool   `json:"full_url"`
	ChannelID uint   `json:"channel_id"`
}

// FetchUpstreamModels pulls model list from upstream provider (/v1/models).
func FetchUpstreamModels(c *fiber.Ctx) error {
	var req fetchModelsReq
	if err := c.BodyParser(&req); err != nil {
		return c.Status(400).JSON(fail("invalid body"))
	}
	req.BaseURL = strings.TrimSpace(req.BaseURL)
	req.Type = strings.TrimSpace(req.Type)
	if req.Type == "" {
		req.Type = model.ChannelTypeOpenAI
	}
	if req.Type != model.ChannelTypeOpenAI && req.Type != model.ChannelTypeClaude {
		return c.Status(400).JSON(fail("type must be openai or claude"))
	}
	if req.BaseURL == "" {
		return c.Status(400).JSON(fail("base_url required"))
	}

	apiKey := strings.TrimSpace(req.APIKey)
	// masked or empty key: load from saved channel
	if apiKey == "" || strings.Contains(apiKey, "***") {
		if req.ChannelID == 0 {
			return c.Status(400).JSON(fail("请填写 API Key，或先保存渠道后再获取"))
		}
		var ch model.Channel
		if err := db.DB.First(&ch, req.ChannelID).Error; err != nil {
			return c.Status(404).JSON(fail("channel not found"))
		}
		keys := ch.GetKeys()
		if len(keys) == 0 {
			return c.Status(400).JSON(fail("渠道未配置 API Key"))
		}
		apiKey = keys[0]
		if req.BaseURL == "" {
			req.BaseURL = ch.BaseURL
		}
		// inherit full_url from saved channel when not explicitly sent as true from form
		if !req.FullURL {
			req.FullURL = ch.FullURL
		}
	} else {
		// multi-line key: first line
		if parts := strings.Split(apiKey, "\n"); len(parts) > 0 {
			apiKey = strings.TrimSpace(parts[0])
		}
	}
	if apiKey == "" {
		return c.Status(400).JSON(fail("api_key required"))
	}

	models, err := fetchUpstreamModelIDs(req.BaseURL, apiKey, req.Type, req.FullURL)
	if err != nil {
		return c.Status(502).JSON(fail(err.Error()))
	}
	return c.JSON(ok(fiber.Map{
		"models": models,
		"count":  len(models),
	}))
}

func fetchUpstreamModelIDs(baseURL, apiKey, channelType string, fullURL bool) ([]string, error) {
	url := buildModelsURL(baseURL, fullURL)
	client := &http.Client{Timeout: 30 * time.Second}
	req, err := http.NewRequest(http.MethodGet, url, nil)
	if err != nil {
		return nil, err
	}
	req.Header.Set("Content-Type", "application/json")
	if channelType == model.ChannelTypeClaude {
		req.Header.Set("x-api-key", apiKey)
		req.Header.Set("anthropic-version", "2023-06-01")
	} else {
		req.Header.Set("Authorization", "Bearer "+apiKey)
	}

	resp, err := client.Do(req)
	if err != nil {
		return nil, fmt.Errorf("请求上游失败: %w", err)
	}
	defer resp.Body.Close()
	body, _ := io.ReadAll(resp.Body)
	if resp.StatusCode >= 400 {
		msg := string(body)
		if len(msg) > 300 {
			msg = msg[:300]
		}
		return nil, fmt.Errorf("上游返回 %d: %s", resp.StatusCode, msg)
	}

	ids := parseModelIDs(body)
	if len(ids) == 0 {
		return nil, fmt.Errorf("上游未返回可用模型，原始响应: %s", truncateStr(string(body), 200))
	}
	sort.Strings(ids)
	return ids, nil
}

func buildModelsURL(baseURL string, fullURL bool) string {
	base := strings.TrimRight(strings.TrimSpace(baseURL), "/")
	if fullURL {
		// BaseURL is a full chat/messages endpoint; derive /models from parent path.
		// e.g. https://x.com/foo/chat/completions -> https://x.com/foo/models
		//      https://x.com/v1/messages -> https://x.com/v1/models
		lower := strings.ToLower(base)
		for _, suffix := range []string{
			"/chat/completions",
			"/v1/chat/completions",
			"/messages",
			"/v1/messages",
		} {
			if strings.HasSuffix(lower, suffix) {
				return base[:len(base)-len(suffix)] + "/models"
			}
		}
		// if already points to models, use as-is
		if strings.HasSuffix(lower, "/models") {
			return base
		}
		// last resort: replace last path segment with models
		if i := strings.LastIndex(base, "/"); i > 0 {
			return base[:i] + "/models"
		}
		return base + "/models"
	}
	lower := strings.ToLower(base)
	if strings.HasSuffix(lower, "/v1") {
		return base + "/models"
	}
	if strings.HasSuffix(lower, "/v1/models") {
		return base
	}
	return base + "/v1/models"
}

func parseModelIDs(body []byte) []string {
	// OpenAI: { "data": [ { "id": "..." } ] }
	var oai struct {
		Data []struct {
			ID string `json:"id"`
		} `json:"data"`
	}
	if err := json.Unmarshal(body, &oai); err == nil && len(oai.Data) > 0 {
		out := make([]string, 0, len(oai.Data))
		seen := map[string]struct{}{}
		for _, m := range oai.Data {
			id := strings.TrimSpace(m.ID)
			if id == "" {
				continue
			}
			if _, ok := seen[id]; ok {
				continue
			}
			seen[id] = struct{}{}
			out = append(out, id)
		}
		return out
	}

	// Claude: { "data": [ { "id": "..." } ] } same shape, or nested
	var generic map[string]any
	if err := json.Unmarshal(body, &generic); err != nil {
		return nil
	}
	if data, ok := generic["data"].([]any); ok {
		out := make([]string, 0, len(data))
		seen := map[string]struct{}{}
		for _, item := range data {
			m, ok := item.(map[string]any)
			if !ok {
				continue
			}
			id, _ := m["id"].(string)
			if id == "" {
				id, _ = m["model"].(string)
			}
			id = strings.TrimSpace(id)
			if id == "" {
				continue
			}
			if _, ok := seen[id]; ok {
				continue
			}
			seen[id] = struct{}{}
			out = append(out, id)
		}
		return out
	}
	// plain string array
	var arr []string
	if err := json.Unmarshal(body, &arr); err == nil {
		return arr
	}
	return nil
}

func truncateStr(s string, n int) string {
	if len(s) <= n {
		return s
	}
	return s[:n] + "..."
}

func channelView(ch model.Channel, fullKey bool, usage channelUsage) fiber.Map {
	key := ch.APIKey
	if !fullKey {
		key = maskKey(key)
	}
	return fiber.Map{
		"id":                ch.ID,
		"name":              ch.Name,
		"type":              ch.Type,
		"base_url":          ch.BaseURL,
		"full_url":          ch.FullURL,
		"api_key":           key,
		"models":            ch.Models,
		"model_mapping":     ch.ModelMapping,
		"status":            ch.Status,
		"weight":            ch.Weight,
		"priority":          ch.Priority,
		"pricing":           ch.Pricing,
		"remark":            ch.Remark,
		"icon":              ch.Icon,
		"response_time":     ch.ResponseTime,
		"test_time":         ch.TestTime,
		"created_at":        ch.CreatedAt,
		"updated_at":        ch.UpdatedAt,
		"total_tokens":      usage.Total,
		"prompt_tokens":     usage.PromptTokens,
		"completion_tokens": usage.CompletionTokens,
		"requests":          usage.Requests,
		"cost_rmb":          usage.CostRMB,
	}
}

func maskKey(key string) string {
	if key == "" {
		return ""
	}
	// multi-line
	parts := strings.Split(key, "\n")
	for i, p := range parts {
		p = strings.TrimSpace(p)
		if len(p) <= 8 {
			parts[i] = "****"
		} else {
			parts[i] = p[:4] + "****" + p[len(p)-4:]
		}
	}
	return strings.Join(parts, "\n")
}

// ---- Tokens ----

func ListTokens(c *fiber.Ctx) error {
	var list []model.Token
	if err := db.DB.Order("id desc").Find(&list).Error; err != nil {
		return c.Status(500).JSON(fail(err.Error()))
	}
	return c.JSON(ok(list))
}

func CreateToken(c *fiber.Ctx) error {
	var req model.Token
	if err := c.BodyParser(&req); err != nil {
		return c.Status(400).JSON(fail("invalid body"))
	}
	if req.Name == "" {
		return c.Status(400).JSON(fail("name required"))
	}
	req.Key = "sk-" + randomHex(24)
	req.Status = model.TokenStatusEnabled
	if err := db.DB.Create(&req).Error; err != nil {
		return c.Status(500).JSON(fail(err.Error()))
	}
	return c.JSON(ok(req))
}

func UpdateToken(c *fiber.Ctx) error {
	id, _ := strconv.Atoi(c.Params("id"))
	var tok model.Token
	if err := db.DB.First(&tok, id).Error; err != nil {
		return c.Status(404).JSON(fail("not found"))
	}
	var req model.Token
	if err := c.BodyParser(&req); err != nil {
		return c.Status(400).JSON(fail("invalid body"))
	}
	tok.Name = req.Name
	tok.Status = req.Status
	tok.ModelLimits = req.ModelLimits
	tok.ExpiredAt = req.ExpiredAt
	if err := db.DB.Save(&tok).Error; err != nil {
		return c.Status(500).JSON(fail(err.Error()))
	}
	return c.JSON(ok(tok))
}

func ResetTokenKey(c *fiber.Ctx) error {
	id, _ := strconv.Atoi(c.Params("id"))
	var tok model.Token
	if err := db.DB.First(&tok, id).Error; err != nil {
		return c.Status(404).JSON(fail("not found"))
	}
	tok.Key = "sk-" + randomHex(24)
	if err := db.DB.Save(&tok).Error; err != nil {
		return c.Status(500).JSON(fail(err.Error()))
	}
	return c.JSON(ok(tok))
}

func DeleteToken(c *fiber.Ctx) error {
	id, _ := strconv.Atoi(c.Params("id"))
	if err := db.DB.Delete(&model.Token{}, id).Error; err != nil {
		return c.Status(500).JSON(fail(err.Error()))
	}
	return c.JSON(ok(nil))
}

func randomHex(n int) string {
	b := make([]byte, n)
	_, _ = rand.Read(b)
	return hex.EncodeToString(b)
}

// ---- Logs ----

func ListLogs(c *fiber.Ctx) error {
	page, _ := strconv.Atoi(c.Query("page", "1"))
	pageSize, _ := strconv.Atoi(c.Query("page_size", "20"))
	if page < 1 {
		page = 1
	}
	if pageSize < 1 || pageSize > 100 {
		pageSize = 20
	}
	q := db.DB.Model(&model.RequestLog{})
	if v := c.Query("token_id"); v != "" {
		q = q.Where("token_id = ?", v)
	}
	if v := c.Query("channel_id"); v != "" {
		q = q.Where("channel_id = ?", v)
	}
	if v := c.Query("model"); v != "" {
		q = q.Where("model = ?", v)
	}
	if v := c.Query("status"); v != "" {
		q = q.Where("status = ?", v)
	}
	if v := c.Query("start"); v != "" {
		if t, err := time.Parse(time.RFC3339, v); err == nil {
			q = q.Where("created_at >= ?", t)
		}
	}
	if v := c.Query("end"); v != "" {
		if t, err := time.Parse(time.RFC3339, v); err == nil {
			q = q.Where("created_at <= ?", t)
		}
	}
	var total int64
	q.Count(&total)
	var list []model.RequestLog
	if err := q.Order("id desc").Offset((page - 1) * pageSize).Limit(pageSize).Find(&list).Error; err != nil {
		return c.Status(500).JSON(fail(err.Error()))
	}
	return c.JSON(ok(fiber.Map{
		"list":      list,
		"total":     total,
		"page":      page,
		"page_size": pageSize,
	}))
}

func GetLog(c *fiber.Ctx) error {
	id, _ := strconv.Atoi(c.Params("id"))
	var log model.RequestLog
	if err := db.DB.First(&log, id).Error; err != nil {
		return c.Status(404).JSON(fail("not found"))
	}
	return c.JSON(ok(log))
}

// ---- Dashboard ----

func Dashboard(c *fiber.Ctx) error {
	// Prefer range mode when start/end provided (used by new dashboard UI).
	startStr := strings.TrimSpace(c.Query("start"))
	endStr := strings.TrimSpace(c.Query("end"))
	if startStr != "" && endStr != "" {
		return dashboardRange(c, startStr, endStr, strings.TrimSpace(c.Query("granularity")))
	}

	now := time.Now()
	todayStart := time.Date(now.Year(), now.Month(), now.Day(), 0, 0, 0, 0, now.Location())
	weekStart := todayStart.AddDate(0, 0, -6)

	type agg struct {
		Requests         int64
		PromptTokens     int64
		CompletionTokens int64
		CostRMB          float64
	}
	var today, total agg
	db.DB.Model(&model.RequestLog{}).
		Select("count(*) as requests, coalesce(sum(prompt_tokens),0) as prompt_tokens, coalesce(sum(completion_tokens),0) as completion_tokens, coalesce(sum(cost_rmb),0) as cost_rmb").
		Where("created_at >= ?", todayStart).
		Scan(&today)
	db.DB.Model(&model.RequestLog{}).
		Select("count(*) as requests, coalesce(sum(prompt_tokens),0) as prompt_tokens, coalesce(sum(completion_tokens),0) as completion_tokens, coalesce(sum(cost_rmb),0) as cost_rmb").
		Scan(&total)

	type dayRow struct {
		Day              string  `json:"day"`
		Requests         int64   `json:"requests"`
		PromptTokens     int64   `json:"prompt_tokens"`
		CompletionTokens int64   `json:"completion_tokens"`
		CostRMB          float64 `json:"cost_rmb"`
	}
	series := make([]dayRow, 0)
	_ = db.DB.Model(&model.RequestLog{}).
		Select("strftime('%Y-%m-%d', created_at) as day, count(*) as requests, coalesce(sum(prompt_tokens),0) as prompt_tokens, coalesce(sum(completion_tokens),0) as completion_tokens, coalesce(sum(cost_rmb),0) as cost_rmb").
		Where("created_at >= ?", weekStart).
		Group("strftime('%Y-%m-%d', created_at)").
		Order("day asc").
		Scan(&series).Error

	var channelCount, tokenCount int64
	db.DB.Model(&model.Channel{}).Count(&channelCount)
	db.DB.Model(&model.Token{}).Count(&tokenCount)

	return c.JSON(ok(fiber.Map{
		"today": fiber.Map{
			"requests":          today.Requests,
			"prompt_tokens":     today.PromptTokens,
			"completion_tokens": today.CompletionTokens,
			"total_tokens":      today.PromptTokens + today.CompletionTokens,
			"cost_rmb":          today.CostRMB,
		},
		"total": fiber.Map{
			"requests":          total.Requests,
			"prompt_tokens":     total.PromptTokens,
			"completion_tokens": total.CompletionTokens,
			"total_tokens":      total.PromptTokens + total.CompletionTokens,
			"cost_rmb":          total.CostRMB,
		},
		"series":        series,
		"channel_count": channelCount,
		"token_count":   tokenCount,
	}))
}

func dashboardRange(c *fiber.Ctx, startStr, endStr, granularity string) error {
	start, err1 := parseFlexibleTime(startStr)
	end, err2 := parseFlexibleTime(endStr)
	if err1 != nil || err2 != nil {
		return c.Status(400).JSON(fail("invalid start/end time"))
	}
	if end.Before(start) {
		start, end = end, start
	}

	type agg struct {
		Requests         int64
		PromptTokens     int64
		CompletionTokens int64
		CostRMB          float64
	}
	var summary agg
	db.DB.Model(&model.RequestLog{}).
		Select("count(*) as requests, coalesce(sum(prompt_tokens),0) as prompt_tokens, coalesce(sum(completion_tokens),0) as completion_tokens, coalesce(sum(cost_rmb),0) as cost_rmb").
		Where("created_at >= ? AND created_at <= ?", start, end).
		Scan(&summary)

	durationMin := end.Sub(start).Minutes()
	if durationMin < 1 {
		durationMin = 1
	}
	totalTokens := float64(summary.PromptTokens + summary.CompletionTokens)
	rpm := float64(summary.Requests) / durationMin
	tpm := totalTokens / durationMin

	// Bucket expression per requested granularity. Falls back to auto:
	// hourly for short ranges, daily for longer.
	const (
		hourExpr  = "strftime('%Y-%m-%d %H:00', created_at)"
		dayExpr   = "strftime('%Y-%m-%d', created_at)"
		// Start-of-week (Monday) as a plain date, so the frontend can parse it.
		weekExpr  = "date(created_at, '-' || ((strftime('%w', created_at) + 6) % 7) || ' days')"
		monthExpr = "strftime('%Y-%m', created_at)"
	)
	var bucketExpr string
	switch granularity {
	case "hour":
		bucketExpr = hourExpr
	case "day":
		bucketExpr = dayExpr
	case "week":
		bucketExpr = weekExpr
	case "month":
		bucketExpr = monthExpr
	default:
		if end.Sub(start) <= 48*time.Hour {
			bucketExpr = hourExpr
		} else {
			bucketExpr = dayExpr
		}
	}

	type seriesRow struct {
		Time             string  `json:"time"`
		Requests         int64   `json:"requests"`
		PromptTokens     int64   `json:"prompt_tokens"`
		CompletionTokens int64   `json:"completion_tokens"`
		TotalTokens      int64   `json:"total_tokens"`
		CostRMB          float64 `json:"cost_rmb"`
	}
	type seriesScan struct {
		Time             string
		Requests         int64
		PromptTokens     int64
		CompletionTokens int64
		CostRMB          float64
	}
	rawSeries := make([]seriesScan, 0)
	_ = db.DB.Model(&model.RequestLog{}).
		Select(bucketExpr+" as time, count(*) as requests, coalesce(sum(prompt_tokens),0) as prompt_tokens, coalesce(sum(completion_tokens),0) as completion_tokens, coalesce(sum(cost_rmb),0) as cost_rmb").
		Where("created_at >= ? AND created_at <= ?", start, end).
		Group(bucketExpr).
		Order("time asc").
		Scan(&rawSeries).Error

	series := make([]seriesRow, 0, len(rawSeries))
	for _, r := range rawSeries {
		series = append(series, seriesRow{
			Time:             r.Time,
			Requests:         r.Requests,
			PromptTokens:     r.PromptTokens,
			CompletionTokens: r.CompletionTokens,
			TotalTokens:      r.PromptTokens + r.CompletionTokens,
			CostRMB:          r.CostRMB,
		})
	}

	type distRow struct {
		ChannelName      string  `json:"channel_name"`
		PromptTokens     int64   `json:"prompt_tokens"`
		CompletionTokens int64   `json:"completion_tokens"`
		TotalTokens      int64   `json:"total_tokens"`
		CostRMB          float64 `json:"cost_rmb"`
	}
	type distScan struct {
		ChannelName      string
		PromptTokens     int64
		CompletionTokens int64
		CostRMB          float64
	}
	rawDist := make([]distScan, 0)
	_ = db.DB.Model(&model.RequestLog{}).
		Select("coalesce(nullif(channel_name,''), 'unknown') as channel_name, coalesce(sum(prompt_tokens),0) as prompt_tokens, coalesce(sum(completion_tokens),0) as completion_tokens, coalesce(sum(cost_rmb),0) as cost_rmb").
		Where("created_at >= ? AND created_at <= ? AND status = ?", start, end, "success").
		Group("coalesce(nullif(channel_name,''), 'unknown')").
		Order("prompt_tokens + completion_tokens desc").
		Scan(&rawDist).Error

	distribution := make([]distRow, 0, len(rawDist))
	for _, r := range rawDist {
		distribution = append(distribution, distRow{
			ChannelName:      r.ChannelName,
			PromptTokens:     r.PromptTokens,
			CompletionTokens: r.CompletionTokens,
			TotalTokens:      r.PromptTokens + r.CompletionTokens,
			CostRMB:          r.CostRMB,
		})
	}

	// ===== Per-model call analytics (proportion / ranking / trend) =====
	type modelStatRow struct {
		Model            string  `json:"model"`
		Count            int64   `json:"count"`
		PromptTokens     int64   `json:"prompt_tokens"`
		CompletionTokens int64   `json:"completion_tokens"`
		TotalTokens      int64   `json:"total_tokens"`
		CostRMB          float64 `json:"cost_rmb"`
	}
	type modelStatScan struct {
		Model            string
		Count            int64
		PromptTokens     int64
		CompletionTokens int64
		CostRMB          float64
	}
	rawModelStats := make([]modelStatScan, 0)
	_ = db.DB.Model(&model.RequestLog{}).
		Select("coalesce(nullif(model,''), 'unknown') as model, count(*) as count, coalesce(sum(prompt_tokens),0) as prompt_tokens, coalesce(sum(completion_tokens),0) as completion_tokens, coalesce(sum(cost_rmb),0) as cost_rmb").
		Where("created_at >= ? AND created_at <= ?", start, end).
		Group("coalesce(nullif(model,''), 'unknown')").
		Order("count desc").
		Scan(&rawModelStats).Error

	modelStats := make([]modelStatRow, 0, len(rawModelStats))
	for _, r := range rawModelStats {
		modelStats = append(modelStats, modelStatRow{
			Model:            r.Model,
			Count:            r.Count,
			PromptTokens:     r.PromptTokens,
			CompletionTokens: r.CompletionTokens,
			TotalTokens:      r.PromptTokens + r.CompletionTokens,
			CostRMB:          r.CostRMB,
		})
	}

	type modelSeriesRow struct {
		Time             string  `json:"time"`
		Model            string  `json:"model"`
		Count            int64   `json:"count"`
		PromptTokens     int64   `json:"prompt_tokens"`
		CompletionTokens int64   `json:"completion_tokens"`
		TotalTokens      int64   `json:"total_tokens"`
		CostRMB          float64 `json:"cost_rmb"`
	}
	type modelSeriesScan struct {
		Time             string
		Model            string
		Count            int64
		PromptTokens     int64
		CompletionTokens int64
		CostRMB          float64
	}
	rawModelSeries := make([]modelSeriesScan, 0)
	_ = db.DB.Model(&model.RequestLog{}).
		Select(bucketExpr+" as time, coalesce(nullif(model,''), 'unknown') as model, count(*) as count, coalesce(sum(prompt_tokens),0) as prompt_tokens, coalesce(sum(completion_tokens),0) as completion_tokens, coalesce(sum(cost_rmb),0) as cost_rmb").
		Where("created_at >= ? AND created_at <= ?", start, end).
		Group(bucketExpr + ", coalesce(nullif(model,''), 'unknown')").
		Order("time asc").
		Scan(&rawModelSeries).Error

	modelSeries := make([]modelSeriesRow, 0, len(rawModelSeries))
	for _, r := range rawModelSeries {
		modelSeries = append(modelSeries, modelSeriesRow{
			Time:             r.Time,
			Model:            r.Model,
			Count:            r.Count,
			PromptTokens:     r.PromptTokens,
			CompletionTokens: r.CompletionTokens,
			TotalTokens:      r.PromptTokens + r.CompletionTokens,
			CostRMB:          r.CostRMB,
		})
	}

	// Per-channel time series (for 消耗分布 area/bar by channel)
	type channelSeriesRow struct {
		Time             string  `json:"time"`
		ChannelName      string  `json:"channel_name"`
		PromptTokens     int64   `json:"prompt_tokens"`
		CompletionTokens int64   `json:"completion_tokens"`
		TotalTokens      int64   `json:"total_tokens"`
		CostRMB          float64 `json:"cost_rmb"`
	}
	type channelSeriesScan struct {
		Time             string
		ChannelName      string
		PromptTokens     int64
		CompletionTokens int64
		CostRMB          float64
	}
	rawChannelSeries := make([]channelSeriesScan, 0)
	_ = db.DB.Model(&model.RequestLog{}).
		Select(bucketExpr+" as time, coalesce(nullif(channel_name,''), 'unknown') as channel_name, coalesce(sum(prompt_tokens),0) as prompt_tokens, coalesce(sum(completion_tokens),0) as completion_tokens, coalesce(sum(cost_rmb),0) as cost_rmb").
		Where("created_at >= ? AND created_at <= ? AND status = ?", start, end, "success").
		Group(bucketExpr + ", coalesce(nullif(channel_name,''), 'unknown')").
		Order("time asc").
		Scan(&rawChannelSeries).Error

	channelSeries := make([]channelSeriesRow, 0, len(rawChannelSeries))
	for _, r := range rawChannelSeries {
		channelSeries = append(channelSeries, channelSeriesRow{
			Time:             r.Time,
			ChannelName:      r.ChannelName,
			PromptTokens:     r.PromptTokens,
			CompletionTokens: r.CompletionTokens,
			TotalTokens:      r.PromptTokens + r.CompletionTokens,
			CostRMB:          r.CostRMB,
		})
	}

	return c.JSON(ok(fiber.Map{
		"requests":          summary.Requests,
		"prompt_tokens":     summary.PromptTokens,
		"completion_tokens": summary.CompletionTokens,
		"total_tokens":      summary.PromptTokens + summary.CompletionTokens,
		"cost_rmb":          summary.CostRMB,
		"rpm":               rpm,
		"tpm":               tpm,
		"series":            series,
		"distribution":      distribution,
		"model_stats":       modelStats,
		"model_series":      modelSeries,
		"channel_series":    channelSeries,
		"start":             start.Format(time.RFC3339),
		"end":               end.Format(time.RFC3339),
	}))
}

func parseFlexibleTime(s string) (time.Time, error) {
	s = strings.TrimSpace(s)
	layouts := []string{
		time.RFC3339,
		time.RFC3339Nano,
		"2006-01-02 15:04:05",
		"2006-01-02T15:04:05",
		"2006-01-02",
	}
	for _, layout := range layouts {
		if t, err := time.ParseInLocation(layout, s, time.Local); err == nil {
			return t, nil
		}
		if t, err := time.Parse(layout, s); err == nil {
			return t, nil
		}
	}
	return time.Time{}, fmt.Errorf("invalid time: %s", s)
}

// ---- Settings ----

func GetSettings(c *fiber.Ctx) error {
	var list []model.Setting
	if err := db.DB.Find(&list).Error; err != nil {
		return c.Status(500).JSON(fail(err.Error()))
	}
	m := map[string]string{}
	for _, s := range list {
		m[s.Key] = s.Value
	}
	return c.JSON(ok(m))
}

func UpdateSettings(c *fiber.Ctx) error {
	var req map[string]string
	if err := c.BodyParser(&req); err != nil {
		return c.Status(400).JSON(fail("invalid body"))
	}
	allowed := map[string]bool{
		model.SettingLogBodyMaxBytes:    true,
		model.SettingPriceMissingPolicy: true,
		model.SettingRequestTimeout:     true,
	}
	for k, v := range req {
		if !allowed[k] {
			continue
		}
		if err := db.SetSetting(k, v); err != nil {
			return c.Status(500).JSON(fail(err.Error()))
		}
	}
	return c.JSON(ok(nil))
}

// ensure gorm import used
var _ = gorm.ErrRecordNotFound
