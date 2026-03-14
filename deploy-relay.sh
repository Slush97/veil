#!/bin/bash
# Deploy a veil relay server.
#
# Usage:
#   ./deploy-relay.sh              # build and run locally on port 4433
#   ./deploy-relay.sh 5555         # custom port
#   ./deploy-relay.sh build-only   # just build the image
#
# To deploy on a cloud VM:
#   1. SSH to your VM
#   2. Install Docker (apt install docker.io)
#   3. Clone the repo and run this script
#   4. Open UDP port 4433 in the firewall/security group
#
# The relay stores nothing on disk. Restarting loses queued offline
# messages, but that's fine — they're ephemeral by design.

set -e

PORT="${1:-4433}"
IMAGE="veil-relay"

if [ "$1" = "build-only" ]; then
    echo "Building relay image..."
    docker build -f Dockerfile.relay -t "$IMAGE" .
    echo "Done. Run with: docker run -d --restart unless-stopped -p ${PORT}:4433/udp --name veil-relay $IMAGE"
    exit 0
fi

echo "Building relay image..."
docker build -f Dockerfile.relay -t "$IMAGE" .

# Stop existing container if running
docker rm -f veil-relay 2>/dev/null || true

echo "Starting relay on UDP port ${PORT}..."
docker run -d \
    --restart unless-stopped \
    -p "${PORT}:4433/udp" \
    --name veil-relay \
    --memory=128m \
    --cpus=0.5 \
    "$IMAGE"

echo ""
echo "Relay running. Connect clients with: relay address <your-public-ip>:${PORT}"
echo "Logs: docker logs -f veil-relay"
echo "Stop: docker rm -f veil-relay"
