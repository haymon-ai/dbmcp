# dbmcp Helm chart

Runs [dbmcp](https://github.com/haymon-ai/dbmcp), a single-binary MCP server for MySQL, MariaDB, PostgreSQL, and SQLite, on Kubernetes over Streamable HTTP.

```bash
helm install dbmcp oci://ghcr.io/haymon-ai/charts/dbmcp --values values.yaml
```

See the [Kubernetes guide](https://dbmcp.haymon.ai/docs/kubernetes) for a minimal `values.yaml`, secrets, Ingress, and probes, and [`values.yaml`](values.yaml) for every value.
