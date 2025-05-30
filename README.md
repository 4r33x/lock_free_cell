
Batch memory reclamation lockfree Cell prototype (its vibe coded, but it passes miri, so use at own risk)

Readers always read lockfree and writing done via Copy-on-write mutation (also lockfree)

Bench from arc_swap for int access: 

| Readers + Writers | rw          | arc-load-store | arc-rcu      | cell-rcu    |
| ----------------- | ----------- | -------------- | ------------ | ----------- |
| 1 + 0             | 424.361µs   | 358.215µs      | 1.590169ms   | 809.988µs   |
| 2 + 0             | 3.664644ms  | 361.941µs      | 4.741752ms   | 843.362µs   |
| 4 + 0             | 8.295304ms  | 796.342µs      | 9.951539ms   | 861.867µs   |
| 8 + 0             | 15.753478ms | 877.567µs      | 18.636788ms  | 958.511µs   |
| 1 + 1             | 3.924601ms  | 22.490283ms    | 43.368284ms  | 5.089487ms  |
| 4 + 1             | 14.79184ms  | 29.925102ms    | 52.132885ms  | 4.975509ms  |
| 4 + 2             | 17.481691ms | 89.987152ms    | 122.561397ms | 12.299256ms |
| 4 + 4             | 32.051115ms | 212.139477ms   | 244.087456ms | 21.9079ms   |
| 8 + 1             | 18.950628ms | 32.444598ms    | 63.466326ms  | 5.49918ms   |
| 8 + 2             | 30.500712ms | 42.689688ms    | 64.769516ms  | 13.04938ms  |
| 8 + 4             | 39.163068ms | 252.820015ms   | 216.655297ms | 21.966512ms |
| 0 + 1             | 397.269µs   | 47.22179ms     | 50.561031ms  | 4.875168ms  |
| 0 + 4             | 17.400305ms | 269.462843ms   | 251.564065ms | 20.846622ms |

Very bad criterion benchmark:

my_lockfreecell_get             time:   [3.6020 ns 3.6161 ns 3.6302 ns]
my_lockfreecell_contended_get   time:   [3.7202 ns 3.7506 ns 3.7786 ns]
my_lockfreecell_update          time:   [18.008 ns 18.077 ns 18.149 ns]
mutex                           time:   [3.3216 ns 3.3310 ns 3.3412 ns]
mutex_contended                 time:   [70.211 ns 71.722 ns 73.184 ns]

rwlock_write                    time:   [3.7093 ns 3.7214 ns 3.7335 ns]
rwlock_contended_read           time:   [36.443 ns 37.853 ns 39.387 ns]
arcswap_contended_get           time:   [3.3405 ns 3.3489 ns 3.3572 ns]
arc_swap_update                 time:   [54.217 ns 54.362 ns 54.516 ns]
arcshift_update                 time:   [18.772 ns 18.829 ns 18.895 ns]
arcshift_contended_get          time:   [69.648 ns 71.027 ns 72.394 ns]
my_spincell_update              time:   [3.4618 ns 3.4708 ns 3.4796 ns]
my_spincell_get                 time:   [3.8248 ns 3.8339 ns 3.8437 ns]
my_spincell_contended_get       time:   [29.100 ns 29.208 ns 29.312 ns]




