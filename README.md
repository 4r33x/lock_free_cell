
Batch memory reclamation lockfree Cell prototype (its vibe coded, but it passes miri, so use at own risk)

Readers always read lockfree and writing done via Copy-on-write mutation (also lockfree)

Writes for large types could be done much faster via reusing allocatons (see src/sz2 and src/sz3)

### Bench from arc_swap for int access: 

| Implementation\R+W   | 0+1       | 0+4       | 1+0     | 1+1      | 2+0     | 4+0     | 4+1      | 4+2      | 4+4       | 8+0     | 8+1      | 8+2      | 8+4       |
|------------------|-----------|-----------|---------|----------|---------|---------|----------|----------|-----------|---------|----------|----------|-----------|
| arc-swap-load-store   | 36.982    | 271.726   | 0.842   | 24.265   | **0.434** | **0.397** | 24.597   | 27.740   | 196.905   | **0.567** | 29.389   | 73.977   | 263.461   |
| arc-swap-rcu          | 36.244    | 237.532   | 1.339   | 37.417   | 4.871   | 10.088  | 43.795   | 75.206   | 216.703   | 19.006  | 48.493   | 57.449   | 201.476   |
| lf-cell-rcu         | 4.701 | **20.541**| **0.715** | **4.449** | 0.685   | 0.882   | **4.813** | **12.521** | **22.146** | 0.802   | **4.830** | **12.322** | **23.015** |
| rwlock               | **0.457** | 17.047    | 0.871   | 4.576    | 3.109   | 9.037   | 11.307   | 17.082   | 31.974    | 18.673  | 19.918   | 31.506   | 41.572    |



### Very bad criterion benchmark:

| Operation                      | Min Time   | Avg Time   | Max Time   |
|-------------------------------|------------|------------|------------|
| my_lockfreecell_get           | 3.6020 ns  | 3.6161 ns  | 3.6302 ns  |
| my_lockfreecell_contended_get | 3.7202 ns  | 3.7506 ns  | 3.7786 ns  |
| my_lockfreecell_update        | 18.008 ns  | 18.077 ns  | 18.149 ns  |
| mutex                         | 3.3216 ns  | 3.3310 ns  | 3.3412 ns  |
| mutex_contended               | 70.211 ns  | 71.722 ns  | 73.184 ns  |
| rwlock_write                  | 3.7093 ns  | 3.7214 ns  | 3.7335 ns  |
| rwlock_contended_read         | 36.443 ns  | 37.853 ns  | 39.387 ns  |
| arcswap_contended_get         | 3.3405 ns  | 3.3489 ns  | 3.3572 ns  |
| arc_swap_update               | 54.217 ns  | 54.362 ns  | 54.516 ns  |
| arcshift_update               | 18.772 ns  | 18.829 ns  | 18.895 ns  |
| arcshift_contended_get        | 69.648 ns  | 71.027 ns  | 72.394 ns  |
| my_spincell_update            | 3.4618 ns  | 3.4708 ns  | 3.4796 ns  |
| my_spincell_get               | 3.8248 ns  | 3.8339 ns  | 3.8437 ns  |
| my_spincell_contended_get     | 29.100 ns  | 29.208 ns  | 29.312 ns  |




