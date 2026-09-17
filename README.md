# tfrobot-client
TFRobot Client - A cross-platform desktop application for A2C-SMCP Computer

## Manager environments

The Manager sign-in screen accepts an environment instead of an arbitrary URL. The desktop client
uses these fixed mappings:

| Environment | Manager API |
|-------------|-------------|
| `staging` | `https://api-staging.turingfocus.cn` |
| `prod` | `https://api.turingfocus.cn` |

Upgrades discard retired Beta session hints and Beta chat preferences. Sign in again using
Staging or Production; Beta credentials are never reused for either environment.
