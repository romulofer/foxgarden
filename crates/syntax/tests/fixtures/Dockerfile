# A friendly comment
FROM eclipse-temurin:21-jre AS runtime

ARG APP_VERSION=1.0.0
ENV APP_HOME=/app

WORKDIR $APP_HOME
COPY target/app.jar app.jar

ENTRYPOINT ["java", "-jar", "app.jar"]
