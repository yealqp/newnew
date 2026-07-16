package openai

import (
        "bufio"
        "bytes"
        "context"
        "encoding/json"
        "fmt"
        "io"
        "net/http"
        "strings"
        "time"
)

type Client struct {
        HTTPClient *http.Client
}

func New(timeoutSec int) *Client {
        if timeoutSec <= 0 {
                timeoutSec = 300
        }
        return &Client{
                HTTPClient: &http.Client{Timeout: time.Duration(timeoutSec) * time.Second},
        }
}

// ChatCompletions posts to upstream OpenAI-compatible chat endpoint.
// If fullURL is true, baseURL is used as-is (complete endpoint).
// Otherwise baseURL is treated as origin and /v1/chat/completions is appended.
func (c *Client) ChatCompletions(ctx context.Context, baseURL, apiKey string, body []byte, stream bool, fullURL bool) (*http.Response, error) {
        url := resolveURL(baseURL, fullURL, "/v1/chat/completions")
        req, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(body))
        if err != nil {
                return nil, err
        }
        req.Header.Set("Content-Type", "application/json")
        req.Header.Set("Authorization", "Bearer "+apiKey)
        if stream {
                req.Header.Set("Accept", "text/event-stream")
        }
        resp, err := c.HTTPClient.Do(req)
        if err != nil {
                return nil, err
        }
        return resp, nil
}

func resolveURL(baseURL string, fullURL bool, defaultPath string) string {
        base := strings.TrimSpace(baseURL)
        if fullURL {
                return strings.TrimRight(base, "/")
        }
        return strings.TrimRight(base, "/") + defaultPath
}

// ParseSSELines reads SSE stream lines yielding data payloads (without "data: " prefix).
func ParseSSELines(r io.Reader, onData func(data string) error) error {
        sc := bufio.NewScanner(r)
        // increase buffer for large chunks
        buf := make([]byte, 0, 64*1024)
        sc.Buffer(buf, 10*1024*1024)
        for sc.Scan() {
                line := sc.Text()
                if line == "" {
                        continue
                }
                if !strings.HasPrefix(line, "data:") {
                        continue
                }
                data := strings.TrimSpace(strings.TrimPrefix(line, "data:"))
                if data == "" {
                        continue
                }
                if err := onData(data); err != nil {
                        return err
                }
        }
        return sc.Err()
}

func EncodeSSE(data string) []byte {
        return []byte("data: " + data + "\n\n")
}

func JSONError(status int, msg string) error {
        return fmt.Errorf("upstream %d: %s", status, msg)
}

func ReadBody(resp *http.Response) ([]byte, error) {
        defer resp.Body.Close()
        return io.ReadAll(resp.Body)
}

func PrettyUpstreamError(body []byte) string {
        var m map[string]any
        if err := json.Unmarshal(body, &m); err == nil {
                if e, ok := m["error"].(map[string]any); ok {
                        if msg, ok := e["message"].(string); ok {
                                return msg
                        }
                }
                if errStr, ok := m["error"].(string); ok {
                        return errStr
                }
        }
        s := string(body)
        if len(s) > 500 {
                return s[:500]
        }
        return s
}
