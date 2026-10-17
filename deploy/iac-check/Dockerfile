# syntax=docker/dockerfile:1.7
#
# Toolbox that validates generated infrastructure code against the *real* upstream artifacts:
#   helm        renders the pinned charts and the generated values
#   kubeconform checks every object against Kubernetes and operator CRD schemas (strict)
#   terraform   init / validate / fmt of the generated modules (OpenTofu behaves the same)
#   kubectl     kustomize build of the generated manifests
# Nothing is mocked: charts, CRDs and providers are downloaded from their official sources.
ARG REGISTRY=docker.io
FROM ${REGISTRY}/alpine/helm:3.16.2 AS helm
FROM ghcr.io/yannh/kubeconform:v0.8.0 AS kubeconform
FROM ${REGISTRY}/hashicorp/terraform:1.9 AS terraform
FROM registry.k8s.io/kubectl:v1.33.0 AS kubectl

FROM ${REGISTRY}/python:3.13-slim-bookworm
COPY --from=helm /usr/bin/helm /usr/local/bin/helm
COPY --from=kubeconform /kubeconform /usr/local/bin/kubeconform
COPY --from=terraform /bin/terraform /usr/local/bin/terraform
COPY --from=kubectl /bin/kubectl /usr/local/bin/kubectl
COPY requirements.txt /opt/iac-check/requirements.txt
# optional build secret `ca`: a PEM bundle for builds behind a TLS-intercepting proxy
RUN --mount=type=secret,id=ca,required=false \
    if [ -f /run/secrets/ca ]; then export PIP_CERT=/run/secrets/ca; fi; \
    pip install --no-cache-dir --require-hashes -r /opt/iac-check/requirements.txt
COPY check.py /opt/iac-check/check.py
# everything the check downloads or learns is under /cache and meant to outlive the container (CI restores the directory): charts pulled once (/cache/charts),
# Terraform providers, Kubernetes schemas for kubeconform, Helm's own cache
ENV TF_PLUGIN_CACHE_DIR=/cache/terraform-plugins HELM_CACHE_HOME=/cache/helm IAC_CACHE=/cache
WORKDIR /work
ENTRYPOINT ["python3", "/opt/iac-check/check.py"]
