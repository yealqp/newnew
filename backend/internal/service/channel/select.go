package channel

import (
        "fmt"
        "math/rand"
        "sort"
        "sync/atomic"

        "github.com/newnew/gateway/internal/db"
        "github.com/newnew/gateway/internal/model"
)

var keyCounter uint64

// Select picks an enabled channel that supports the model, by priority then weight.
func Select(modelName string) (*model.Channel, error) {
        var channels []model.Channel
        if err := db.DB.Where("status = ?", model.ChannelStatusEnabled).Find(&channels).Error; err != nil {
                return nil, err
        }
        candidates := make([]model.Channel, 0)
        for _, ch := range channels {
                if ch.SupportsModel(modelName) {
                        candidates = append(candidates, ch)
                }
        }
        if len(candidates) == 0 {
                return nil, fmt.Errorf("no available channel for model %s", modelName)
        }

        // sort priority DESC
        sort.SliceStable(candidates, func(i, j int) bool {
                return candidates[i].Priority > candidates[j].Priority
        })
        topPriority := candidates[0].Priority
        top := make([]model.Channel, 0)
        for _, ch := range candidates {
                if ch.Priority == topPriority {
                        top = append(top, ch)
                }
        }
        if len(top) == 1 {
                return &top[0], nil
        }
        // weighted random
        var total uint
        for _, ch := range top {
                w := ch.Weight
                if w == 0 {
                        w = 1
                }
                total += w
        }
        r := uint(rand.Intn(int(total)))
        var acc uint
        for i := range top {
                w := top[i].Weight
                if w == 0 {
                        w = 1
                }
                acc += w
                if r < acc {
                        return &top[i], nil
                }
        }
        return &top[0], nil
}

// PickKey returns a key from multi-key channel (round-robin).
func PickKey(ch *model.Channel) string {
        keys := ch.GetKeys()
        if len(keys) == 0 {
                return ch.APIKey
        }
        if len(keys) == 1 {
                return keys[0]
        }
        n := atomic.AddUint64(&keyCounter, 1)
        return keys[int(n)%len(keys)]
}

// ListEnabledModels returns unique model names from enabled channels.
func ListEnabledModels() []string {
        var channels []model.Channel
        _ = db.DB.Where("status = ?", model.ChannelStatusEnabled).Find(&channels).Error
        set := map[string]struct{}{}
        for _, ch := range channels {
                for _, m := range ch.GetModels() {
                        set[m] = struct{}{}
                }
        }
        out := make([]string, 0, len(set))
        for m := range set {
                out = append(out, m)
        }
        sort.Strings(out)
        return out
}
