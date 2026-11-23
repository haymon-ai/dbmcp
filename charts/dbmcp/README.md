# dbmcp Helm chart

Runs [dbmcp](https://github.com/haymon-ai/dbmcp), the privacy first MCP server for SQL databases, on Kubernetes over Streamable HTTP.

```bash
helm install dbmcp oci://ghcr.io/haymon-ai/charts/dbmcp --values values.yaml
```

See the [Kubernetes guide](https://dbmcp.haymon.ai/kubernetes) for a minimal `values.yaml`, secrets, Ingress, and probes, and [`values.yaml`](values.yaml) for every value.
